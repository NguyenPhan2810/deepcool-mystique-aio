# mystique-daemon

The headless owner of the DeepCool MYSTIQUE AIO LCD. It opens the panel once, runs the
machine-info init, streams `0x01` telemetry — 1 Hz by default, 30 Hz measured clean — and
serves the Tauri-shaped control
API over a local socket. `apps/mystique-ui` is a client of this daemon: it holds no USB
handle and runs no sampler. The CLI still opens the device directly.

The daemon is deliberately boring about failure: a missing panel, an access-denied open,
a sleep/resume disconnect or a write timeout never exits it. It drops the handle, backs
off, reopens, and re-runs init.

## Build and run

```sh
cd rust
cargo run -p mystique-daemon --                     # 1 Hz, rotation 0, labels "--"
cargo run -p mystique-daemon -- --interval-ms 500
cargo run -p mystique-daemon -- --interval-ms 33   # 30 Hz, measured clean
cargo run -p mystique-daemon -- --rotation 2 --cpu "Intel Core i9-14900K" --gpu "RTX 4070"
```

The binary is `mystique-daemon`. The device is single-owner: stop `MystiqueService` on
Windows and close `DeepCool.exe` before it can acquire the panel. The daemon reports the
access-denied state and retries; it never stops the vendor service itself.

| Flag | Default | Meaning |
|---|---|---|
| `--interval-ms N` | `1000` | Telemetry period, 8–60000 ms. 33 ms is 30 Hz. |
| `--rotation N` | settings file, else `0` | Rotation used by the machine-info init (0–3). Passing the flag wins for this start only. |
| `--cpu NAME` | settings file, else `--` | ASCII label for `0x15` and the init handshake, max 39 bytes. |
| `--gpu NAME` | settings file, else `--` | ASCII label for `0x16`, max 39 bytes. |
| `--endpoint VALUE` | platform | Override the socket. Windows: pipe name; Unix: absolute path. |
| `--service` | off | Windows service mode. Not implemented; exits 2 with an explanation. |

## Endpoints

- **Windows**: `\\.\pipe\mystique-control-v1` (local named pipe, one instance per client).
- **Linux**: `$XDG_RUNTIME_DIR/mystique/control.sock`, falling back to
  `/tmp/mystique-<uid>/control.sock`. The directory is `0700`, the socket `0600`.
- The socket is local-only. There is no TCP listener, no authentication layer beyond the
  filesystem permission, and no remote access.

A single-instance lock (`daemon.lock` in the same per-user directory, or
`%LOCALAPPDATA%\mystique\daemon.lock` on Windows) is taken **before** the socket or the
device. A second daemon exits with `another mystique-daemon is already running`.

## Settings file

Settings survive a restart in one JSON file, read **before** the device init and written
after every successful settings command:

- Windows: `%LOCALAPPDATA%\mystique\settings.json`
- Linux: `$XDG_CONFIG_HOME/mystique/settings.json`, or
  `~/.config/mystique/settings.json` when `XDG_CONFIG_HOME` is unset

```json
{
  "version": 1,
  "brightness": 70,
  "rotation": 0,
  "gyro": false,
  "idle_screen": true,
  "rgb_sync": true,
  "screen_restore_brightness": 70,
  "recording": { "cpu_clock": true, "cpu_temp": true },
  "areas": { "main": 0, "style": 0, "aux": 0 },
  "media_timing": { "time": 0, "animation": 0 },
  "labels": { "cpu": "--", "gpu": "--" },
  "fan_io": { "cpu": "cpu", "pump": "pump" }
}
```

`version` is `1`. The file is written atomically — the new body goes to
`settings.json.tmp` and is renamed over the old file — after a successful
`set_brightness`, `set_rotation`, `set_gyro`, `set_screen`, `set_idle_screen`,
`set_rgb_sync`, `set_recording`, `set_areas`, `set_media_timing`, `set_labels` or
`set_fan_io`. A failed command writes nothing. A write failure is printed to stderr and
does not fail the command.

A missing file is a fresh install: defaults, no message. A file that cannot be read,
parsed or applied (bad JSON, unknown `version`, out-of-range value, bad label) is
reported once on stderr and ignored, and the daemon starts on its defaults. Deleting the
file is the reset.

**The telemetry stream rate is not persisted.** It stays a session choice (and a CLI
flag), because it is a host timing preference rather than panel state. `--rotation`,
`--cpu` and `--gpu` override the file for the current start only; without the flags the
file wins, which is why they have no defaults.

`device_state` reports the loaded values from its first call, and the reconnect/init
handshake carries the saved `0x02` display config, `0x05` recording options, `0x04`
areas and the two labels.

## Wire format

Newline-delimited UTF-8 JSON. One message per line. A client sends a request; the daemon
answers it and broadcasts unsolicited events.

```text
-> {"id":1,"cmd":"set_brightness","args":{"brightness":40}}
<- {"id":1,"ok":true,"data":{"packets":[...],"settings":{...},"message":"..."}}
<- {"event":"telemetry","frame":{...}}
<- {"event":"state","state":{...}}
```

`id` is chosen by the client and echoed. The daemon does not require it to be unique, but
a client that multiplexes should use a counter. Unknown commands and bad arguments are
answered with `ok:false`; they never close the connection.

### Request

```json
{ "id": 1, "cmd": "set_brightness", "args": { "brightness": 40 } }
```

### Response

Success carries `data`; failure carries `error`. Exactly one is present.

```json
{ "id": 1, "ok": true, "data": { ... } }
{ "id": 1, "ok": false, "error": { "code": "not_open", "message": "...", "hint": "...", "packets": [] } }
```

`CommandError` and `PacketRecord` are byte-for-byte the Tauri shapes. The codes are
`not_open`, `not_found`, `access_denied`, `timeout`, `stall`, `bad_command`,
`disconnected`, `rejected`, `protocol`, `range`, `media`, `upload`, `io`, `wrong_kind`,
`internal`, plus two daemon-only codes: `unknown_command` and `busy` (the bounded worker
queue is full).

### Events

```json
{ "event": "telemetry", "frame": {
    "cpu_temperature_c": 41, "cpu_usage_percent": 12, "memory_load_percent": 55,
    "gpu_temperature_c": null, "cpu_clock_mhz": 4300,
    "missing": ["gpu_temperature_c"], "source": "vendor sensor pipe" } }
```

A sensor that cannot be read is `null` and named in `missing`. The wire frame still sends
zero there, because the protocol has no "unknown"; nothing in the event pretends it is a
reading.

```json
{ "event": "state", "state": { "device_open": true, "device_present": true,
    "service_running": false, "service_supported": true, "service_output": "...",
    "last_error": null, "rotation": 2, "settings": { ... },
    "telemetry": { "running": true, "frames_sent": 42, "interval_ms": 1000, ... } } }
```

`state` is the Tauri `DeviceState` plus an optional `telemetry` extension. `state` is
sent after every command and on every connection transition. `telemetry` is sent after
each successful frame.

## Commands

Arguments and return DTOs match the Tauri commands; `set_screen` is the deliberate rename
of the UI's `set_screen_power`, with the argument `enabled`.

| Command | Arguments | Returns |
|---|---|---|
| `status` | none | `StatusDto` (live `0x10` when open) |
| `device_state` | none | `DeviceState` |
| `open_device` | none | `OpenResult` |
| `close_device` | none | `DeviceState` |
| `set_brightness` | `brightness: 0..=100` | `CommandOutcome` |
| `set_rotation` | `rotation: 0..=3` | `CommandOutcome` |
| `set_gyro` | `enabled: bool` | `CommandOutcome` |
| `set_screen` | `enabled: bool` | `CommandOutcome` |
| `set_idle_screen` | `enabled: bool` | `CommandOutcome` (`0x02[0]`) |
| `set_rgb_sync` | `enabled: bool` | `CommandOutcome` (`0x02[3]`) |
| `set_recording` | `cpu_clock: bool`, `cpu_temp: bool` | `CommandOutcome` (`0x05`, then one redraw frame) |
| `set_mode` | `mode: 1\|2\|3` (Monitor/Media/Recording) | `CommandOutcome` (`0x03`) |
| `set_fan_io` | `cpu: "cpu"\|"pump"\|"none"`, `pump: …` | `CommandOutcome`; host routing, one redraw frame, no setting packet |
| `set_areas` | `main: 0..=15`, `style: 0..=2` (default `0`), `aux: 0..=15` | `CommandOutcome` |
| `set_media_timing` | `time: 0..=2`, `animation: 0..=2` | `CommandOutcome` |
| `set_labels` | `cpu: string?`, `gpu: string?` | `CommandOutcome` |
| `detected_labels` | none | `DetectedLabels`: `{ "cpu": [...], "gpu": [...] }`; host probe, no device needed, empty lists on failure |
| `raw_packet` | `command: u8`, `payload: bytes[0..=39]`, `pipe: "init"\|"data"`, `dry_run: bool` | `RawPacketOutcome` |
| `erase_images` | none | `CommandOutcome` |
| `plan_upload` | `path: string`, `fit?: "cover"\|"contain"` | `UploadPlanSummary` |
| `upload_image` | `path: string`, `fit?` | `CommandOutcome` |
| `upload_gif` | `path: string`, `fit?` | `CommandOutcome` |
| `start_stream` | none | `TelemetryStatus` |
| `stop_stream` | none | `TelemetryStatus` |
| `set_stream_rate` | `hz: 1..=120` | `TelemetryStatus`; applies on the next tick, no device traffic |
| `shutdown` | none | `ShutdownAck`; reply first, then the clean stop |

Semantics worth knowing:

- **The rate is a host property.** No command carries it; the panel is never told. The
  link is not the limit — the panel acknowledges a frame in ~0.5 ms, so 30 Hz costs 2.9
  KB/s, about 0.19% of a 12 Mbit/s link. On Windows the clean ceiling is 30 Hz because the
  default timer resolution is 15.625 ms; slower requests are exact. Measurements, before
  and after, are in `re/findings/20-stream-rate.md`.
- **The vendor sensor pipe pushes once per ~1.1 s**, so it is read on its own thread and
  the sampler copies the newest snapshot. Blocking on that push was the old 1 Hz limit.
  Numbers therefore change at the sensor rate even when the frame rate is 30 Hz; the
  frame rate buys smoothness and panel-side animation, not fresher readings.
- `TelemetryStatus` carries the evidence: `frames_sent`, `frames_failed`,
  `measured_interval_ms` (achieved period), `last_sample_us`, `last_transfer_us`,
  `max_transfer_us`.
- **Screen off is brightness 0.** `set_screen(false)` writes brightness 0 in the same
  five-byte `0x02` payload; `set_screen(true)` restores the last non-zero value (70 until
  one is set). `0x02` byte 0 is the idle screen display, not a screen switch.
- `set_idle_screen` writes `0x02` byte 0 (`01` enabled, `00` disabled). It does not blank
  the panel and is never presented as screen power.
- `set_rgb_sync` writes `0x02` byte 3: **`00` follows the motherboard, `01` keeps the
  panel's own effect** (observed on the panel 2026-11-19; the capture in findings 18
  correlated the byte but had the direction inverted). The default is synced, which is
  why the captured init block carries `00` there. The RGB temperature warning has no LCD
  command.
- `set_recording` writes `0x05` as `[cpu_clock, cpu_temp]`, then pushes one immediate `0x01`
  frame because the graphs only draw with data. The frame does not change stream counters.
- `set_mode` writes `0x03` as `[1|2|3]`. Stored images still hold the panel until `0x14`
  erase; selecting Monitor does not return the dashboard on its own.
- `set_fan_io` chooses which sensor feeds each RPM dial. **It is host routing, not a
  device command**: `0x01` carries one number per dial (21..22 first, 24..25 second) and
  nothing about where it came from. Changing the routing pushes one frame and no setting
  packet. `none` sends 0, which the panel draws as 0 RPM.
- `set_labels` keeps the labels as typed; `detected_labels` lists what the host reports so
  the UI can offer names instead of a text field. Detection is a host probe (`sysinfo` for
  the CPU, PowerShell's `Win32_VideoController` or `lspci -mm` for the GPUs) and is
  answered by the connection handler, **not** the device worker: a slow PowerShell start
  must not stall the frame cadence. It needs no open device; a probe that fails answers
  with empty lists.
- `set_rotation` always clears the gyro in the same payload; the panel ignores rotation
  while the gyro is enabled.
- `set_areas` sends `0x04` and then one immediate `0x01` frame, because a style byte only
  renders once a frame arrives. The frame reuses the daemon's sampler and does not change
  the stream's counters or cadence; if it fails, the layout write still succeeds and the
  message says the redraw could not be sent.
- An upload attempt **stops the telemetry stream** before the first transfer: entering
  image mode is a one-way door per connection and a `0x01` frame after a partial upload
  is not a state we can reason about. Call `start_stream` explicitly afterwards.
- `close_device` stops the stream and turns automatic reconnection off. `open_device`
  turns it back on but does not restart the stream; `start_stream` does.
- `plan_upload` needs no open device; the two upload commands do.
- `erase_images` sends `0x14` alone with an empty payload on the init pipe. It is
  destructive and has no read-back; it is the verified route from stored media back to
  machine info. It needs an open device.
- `raw_packet` builds the exact 48-byte request and, unless `dry_run` is set, sends it on
  the chosen pipe. The daemon never computes a checksum in the client. A send returns the
  response, the result code as data, and the `PacketRecord` to append to the log. A dry run
  needs no open device. Unknown commands can change device state; `0xA1` starts a panel test
  pattern sustained by traffic.
- `status` performs a live `0x10` read and returns an error with the attempted packet if
  it fails. `device_state` never touches USB.
- `shutdown` is the daemon's own stop. It answers `{ "stopping": true, "message": "…" }`
  first, waits for that line to reach the socket, then stops the stream, releases the
  panel, removes the Unix socket and exits. A second request before the process exits is
  answered the same way; after it exits, the connection is simply gone.

## Ownership and recovery

- The panel is opened once and held for the process lifetime. Every USB operation goes
  through one worker thread, so no two transfers interleave.
- On a transport failure — disconnect, timeout, stall, access denied, `ERROR_BAD_COMMAND`
  — the handle is dropped, the stream intent is kept, and the retry is scheduled with
  exponential backoff (1 s, 2 s, 4 s … capped at 30 s). A timeout is treated as a
  possible wedge and waits at least 30 s. The machine-info init is re-run after every
  successful reacquire.
- A device **rejection** (a non-zero result code) is not a lost device: the error is
  returned with its packet record, the handle stays open, and the next frame tries again.
  Range and protocol errors never affect the link.
- `close_device` is the only way to stop automatic reconnection.
- On `Ctrl+C`, `SIGINT`, `SIGTERM` or a `shutdown` request: stop the stream, write
  brightness 0 (a defined, deliberately dark state), close the handle, remove the Unix
  socket. A `shutdown` request is acknowledged before any of that begins.

## Clients, disconnects and reconnects

There is no handshake. Connect, send requests, read lines. Several clients may be
connected at once; each gets its own responses and the same broadcast events.

A client that disappears is noticed when its socket closes; its writer and reader tasks
end and its subscription is dropped. Nothing else is affected.

A client that falls behind is disconnected rather than served stale data: events use a
bounded broadcast channel, and a lagging receiver is dropped. A client that stops reading
is also dropped after a 10-second write timeout. Responses are separately bounded, so a
slow client cannot grow the daemon's memory.

To reconnect: open the endpoint again, send `device_state` (and `status` if the live
`0x10` reading matters), then resume. Events are **not** replayed, so the state snapshot
is the way to catch up. A replaced daemon process is indistinguishable from a fresh
connection; the same procedure applies.

## Tests

No hardware is opened. Tests cover the IPC round trip, framing across split reads,
unknown commands, the slow-client drop, an end-to-end request over the real named pipe /
Unix socket, a `shutdown` request that answers and then stops the server, and the whole
daemon state machine against `Panel` / `DeviceProvider` / `HostEnvironment` fakes:

```sh
cargo test -p mystique-ipc -p mystique-daemon
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Packaging

- `packaging/linux/mystique-daemon.service` — systemd user unit, restart on failure.
  The udev rule beside it (`packaging/linux/99-mystique.rules`) is what grants the
  seated user access to `3633:0009`.
- `packaging/windows/README.md` — running it as a service; the native wrapper is
  not implemented in this pass.

## Next step

`apps/mystique-ui` is re-pointed at this daemon: `src/daemon/` holds a small client that
maps `invoke("name", args)` onto a `Request`, reads the DTOs from `mystique-ipc`, and
forwards `Event`s to the existing sections. A UI started while the daemon is absent
spawns it as a child, and closing that window leaves the daemon running.
