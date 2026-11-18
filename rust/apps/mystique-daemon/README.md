# mystique-daemon

The headless owner of the DeepCool MYSTIQUE AIO LCD. It opens the panel once, runs the
machine-info init, streams `0x01` telemetry at 1 Hz, and serves the Tauri-shaped control
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
cargo run -p mystique-daemon -- --rotation 2 --cpu "Intel Core i9-14900K" --gpu "RTX 4070"
```

The binary is `mystique-daemon`. The device is single-owner: stop `MystiqueService` on
Windows and close `DeepCool.exe` before it can acquire the panel. The daemon reports the
access-denied state and retries; it never stops the vendor service itself.

| Flag | Default | Meaning |
|---|---|---|
| `--interval-ms N` | `1000` | Telemetry period, 100–60000 ms. |
| `--rotation N` | `0` | Rotation used by the machine-info init (0–3). |
| `--cpu NAME` | `--` | ASCII label for `0x15` and the init handshake, max 39 bytes. |
| `--gpu NAME` | `--` | ASCII label for `0x16`, max 39 bytes. |
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
| `set_areas` | `main: 0..=15`, `style: 0..=2` (default `0`), `aux: 0..=15` | `CommandOutcome` |
| `set_media_timing` | `time: 0..=2`, `animation: 0..=2` | `CommandOutcome` |
| `set_labels` | `cpu: string?`, `gpu: string?` | `CommandOutcome` |
| `erase_images` | none | `CommandOutcome` |
| `plan_upload` | `path: string`, `fit?: "cover"\|"contain"` | `UploadPlanSummary` |
| `upload_image` | `path: string`, `fit?` | `CommandOutcome` |
| `upload_gif` | `path: string`, `fit?` | `CommandOutcome` |
| `start_stream` | none | `TelemetryStatus` |
| `stop_stream` | none | `TelemetryStatus` |
| `shutdown` | none | `ShutdownAck`; reply first, then the clean stop |

Semantics worth knowing:

- **Screen off is brightness 0.** `set_screen(false)` writes brightness 0 in the same
  five-byte `0x02` payload; `set_screen(true)` restores the last non-zero value (70 until
  one is set). `0x02` byte 0 is not a screen switch and is never presented as one.
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

- `rust/packaging/linux/mystique-daemon.service` — user unit, restart on failure.
- `rust/packaging/windows/README.md` — running it as a service; the native wrapper is
  not implemented in this pass.

## Next step

`apps/mystique-ui` is re-pointed at this daemon: `src/daemon/` holds a small client that
maps `invoke("name", args)` onto a `Request`, reads the DTOs from `mystique-ipc`, and
forwards `Event`s to the existing sections. A UI started while the daemon is absent
spawns it as a child, and closing that window leaves the daemon running.
