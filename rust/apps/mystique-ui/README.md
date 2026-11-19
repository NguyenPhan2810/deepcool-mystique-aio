# mystique-ui

A small Tauri 2 desktop control panel for the DeepCool MYSTIQUE AIO LCD. It is a **client
of `mystique-daemon`**: it never opens the USB device, never keeps a session, and never
samples the host. Every control sends one request over the daemon's control socket and
renders what the daemon broadcasts.

That split is the point: the panel keeps streaming telemetry, and keeps its settings,
after this window is closed.

This is a **test surface**, not a polished product. Each change is listed here with what
has actually been seen on the panel, so nothing is presented as proven that was not.

## Build and run

Requirements:

- Rust **1.90 or newer** (Tauri 2 requires it; the rest of the workspace is 1.85)
- the MSVC toolchain
- WebView2 runtime on Windows 11 (preinstalled; the app needs no separate install)
- **no Node.js, no npm, no Tauri CLI, no bundler**

```sh
cd rust
cargo build -p mystique-ui -p mystique-daemon
.\target\debug\mystique-ui.exe
```

`mystique-ui` starts `mystique-daemon` itself when nothing is listening, so one command is
enough for development. To run the pair deliberately, in two terminals:

```sh
cd rust
cargo run -p mystique-daemon -- --cpu "Intel Core i9-14900K" --gpu "RTX 4070"
cargo run -p mystique-ui
```

The daemon prints its endpoint and period on stderr and holds the panel until Ctrl+C. The
UI attaches to it and never terminates it (it did not start it). Close the window and the
stream keeps running; reopen the window and the state is there.

The UI is a Windows GUI-subsystem binary in **all** profiles, so launching it never opens a
console. When the UI starts the daemon itself, the daemon's stderr goes to
`%LOCALAPPDATA%\mystique\daemon.log` (Linux: `$XDG_STATE_HOME/mystique/daemon.log`), which
is where a failed start is diagnosed. The log is appended, not rotated.

The frontend is static files in `ui/`: `index.html`, split stylesheets in `styles/`, and
ES modules in `js/`. Fonts are local WOFF2 files in `ui/fonts/`. `tauri.conf.json` sets
`build.frontendDist` to `ui`, so the files are embedded in the binary through
`tauri-build` + the `custom-protocol` feature. There is no dev server and no
`beforeBuildCommand`; `cargo build` is the complete build. `build.rs` fingerprints the
`ui/` tree, so a UI-only edit also triggers a rebuild of the embedded assets.

The interface follows the **official DeepCool app's device page** as its information
architecture, rendered in the approved Instrument Console material: settings on the left,
a live panel preview on the right, and a Monitor / Media / Recording mode selector. The
vendor's complete control surface is present: the wired controls send real commands, and
the controls that produce no LCD traffic stay interactive with one short inline phrase
naming the subsystem that owns them. No control is hidden and no `NO CMD` badge is used.
The window opens at 1440×900. Inter ships locally as a WOFF2 asset; every number and the
packet log use the local `Cascadia Mono → Consolas → ui-monospace` stack, and nothing is
fetched at runtime. The packet log is a right-hand drawer in the unused margin, closed by
default and streaming while closed. A **Raw packet** disclosure at the bottom of the stage
can build, dry-run and send any command byte the protocol crate accepts; the daemon builds
the 48-byte packet, so the UI never computes a checksum.

## Who holds the device

One process, one owner. That process is the daemon. This window:

- opens no USB handle and sends no packet;
- starts no telemetry thread and takes no host sample;
- asks the daemon to open, release, configure or stream;
- shows the daemon, handle, vendor service and stream as status-bar indicators; the
  stream word is the counted frame rate, and the bar carries no rate control;
- keeps the endpoint, health sentence, session controls and stream counters behind a
  closed **Session** disclosure under the device header.

On start, if the endpoint is absent, the app spawns `mystique-daemon` as a child, waits
up to three seconds for the socket, then connects. That spawn is a convenience, not a
lifetime claim: on exit this window drops its handle to the child and **never terminates
it**. The daemon keeps the panel and the stream for the next window. The only way this
window stops a daemon is the explicit **Stop daemon** control in the session disclosure, which
sends a `shutdown` request and says that the panel will be released and telemetry will
stop.
The daemon's single-instance lock makes a redundant spawn harmless: the duplicate exits
before it touches the socket or the device.

The binary is looked up beside `mystique-ui` first (`target/debug` in a workspace build)
and on `PATH` otherwise. `MYSTIQUE_ENDPOINT` overrides the endpoint for both processes and
is passed to the child as `--endpoint`.

### The vendor service is not the daemon

`MystiqueService` is DeepCool's telemetry service, and it also wants the USB interface.
While it runs, the daemon's open returns access-denied and the session disclosure says so.
`Stop vendor service` and `Start vendor service` run `sc.exe` on **`MystiqueService`**;
they do not touch the daemon, and the UI says so next to the buttons. Stopping the vendor
service needs elevation. The exact `sc.exe` output is shown inline; it is never
paraphrased.

## IPC contract

`daemon_info`, `daemon_retry` and `daemon_stop` are local admin commands; `daemon_stop`
forwards the daemon's `shutdown` request. Every other command is one request on the
daemon socket, and the daemon's DTOs are deserialised straight into the shared types
from `mystique-ipc`.

| UI command (JS-facing) | Daemon command | Notes |
|---|---|---|
| `status` | `status` | live `0x10` read when open (advisory) |
| `device_state` | `device_state` | never touches USB |
| `open_device` | `open_device` | "let the daemon own it" |
| `close_device` | `close_device` | "let the daemon release it"; stops the stream |
| `set_brightness` | `set_brightness` | `{ brightness }` |
| `set_rotation` | `set_rotation` | `{ rotation }`; clears the gyro in the same payload |
| `set_gyro` | `set_gyro` | `{ enabled }` |
| `set_mode` | `set_mode` | `{ mode }`: `1` Monitor, `2` Media, `3` Recording; `0x03` |
| `set_fan_io` | `set_fan_io` | `{ cpu, pump }`: `cpu`\|`pump`\|`none`; host routing for the two RPM dials, one redraw frame, no setting packet |
| `set_idle_screen` | `set_idle_screen` | `{ enabled }`; `0x02[0]`, not screen power |
| `set_rgb_sync` | `set_rgb_sync` | `{ enabled }`; `0x02[3]` |
| `set_rgb_mode` | `set_rgb_mode` | `{ mode }`: `motherboard`\|`temperature`\|`edge`; interlocked, one lighting source |
| `set_recording` | `set_recording` | `{ cpu_clock, cpu_temp }`; `0x05`, then one redraw frame |
| `set_screen_power` | `set_screen` | UI `{ lit }` → daemon `{ enabled }`; off is brightness 0 |
| `erase_images` | `erase_images` | `0x14`, destructive, no read-back |
| `set_areas` | `set_areas` | `{ main, style, aux }` |
| `set_media_timing` | `set_media_timing` | `{ time, animation }` |
| `set_labels` | `set_labels` | `{ cpu, gpu }`, nulls allowed |
| `detected_labels` | `detected_labels` | `{}`; `{ cpu: [...], gpu: [...] }` for the label pickers, empty lists when detection finds nothing |
| `raw_packet` | `raw_packet` | `{ command, payload, pipe, dry_run }`; daemon builds the 48-byte packet |
| `start_telemetry` | `start_stream` | rename only |
| `stop_telemetry` | `stop_stream` | rename only |
| `set_stream_rate` | `set_stream_rate` | `{ hz }`, `1..=120`; still implemented by the daemon, no longer exposed by the UI |
| `telemetry_status` | `device_state` | projects the `telemetry` extension |
| `plan_upload` | `plan_upload` | `{ path, fit }`; caches the plan in the daemon |
| `upload_image` | `upload_image` | `{ path, fit }`; stops the stream first |
| `upload_gif` | `upload_gif` | `{ path, fit }` |
| `stop_service` | — | local `sc.exe stop MystiqueService` |
| `start_service` | — | local `sc.exe start MystiqueService` |
| `daemon_info` | — | current link, endpoint, process provenance, health |
| `daemon_retry` | — | start if absent, reconnect, return the same payload |
| `daemon_stop` | `shutdown` | the daemon acknowledges, then releases the panel and exits |

`set_screen_power` and `start_telemetry`/`stop_telemetry` keep their names on this side so
the frontend did not have to change; the renames live in the command bodies.

`set_stream_rate` changes the daemon's host-side period. It still exists in the daemon and
still sends nothing to the panel, but the UI no longer exposes it: the period is whatever
the daemon was started with. The status bar keeps the counted rate (`30.1 HZ`) and the
session sheet keeps the counters and the achieved row. Opening the session sheet takes one
status snapshot; the sheet does not poll, so inspecting it cannot perturb the timing.

`set_mode` sends `0x03` with `1` Monitor, `2` Media or `3` Recording. The mode buttons
switch the view as soon as they are clicked, but their active state changes only after the
daemon accepts the write. A failed send leaves the last accepted button active and appears
in the existing `app-error` node. A mode write does not erase stored content; the erase
control remains the way back from an uploaded image.

Success payloads are the IPC types: `StatusDto`, `DeviceState`, `OpenResult`,
`CommandOutcome`, `TelemetryStatus`, `UploadPlanSummary`. On failure the daemon's
`CommandError` is passed through unchanged:

```json
{
  "code": "not_open | not_found | access_denied | timeout | stall | bad_command | disconnected | rejected | protocol | range | media | upload | io | wrong_kind | internal | unknown_command | busy",
  "message": "human-readable, verbatim for sc.exe failures",
  "hint": "optional next step, or null",
  "packets": [PacketRecord]
}
```

Three codes exist only here:

| Code | Meaning |
|---|---|
| `daemon_unavailable` | the endpoint is absent, or the client could not reach it |
| `daemon_timeout` | the request was written and no answer arrived in 10 s |
| `service_stop_failed`, `service_start_failed` | `sc.exe` refused; the message is verbatim |

`ServiceResult` for the two admin actions is
`{ "output": "verbatim sc.exe text", "device": DeviceState | null }`. A `null` device is
the daemon being unreachable, and the session disclosure carries the reason. The `sc.exe`
summary in the strip opens the verbatim output; it is never paraphrased.

### Events

The backend emits four Tauri events. The frontend never polls.

| Event | Payload | Rendered by |
|---|---|---|
| `daemon-connection` | `{ phase, endpoint, spawned_by_ui, launch_error, health }` | status bar, session disclosure, log LIVE lamp |
| `daemon-state` | `DeviceState` (with the `telemetry` extension) | status bar, session disclosure, every settings group, panel preview |
| `daemon-status` | `StatusDto` | rotation source, preview `0x10` line |
| `daemon-telemetry` | `FrameValues` | session counters, preview, recording graph |

`health` is computed in Rust, not JavaScript:

```json
{ "code": "ok | connecting | waiting | daemon_unavailable | daemon_timeout | device_absent | vendor_conflict",
  "headline": "Panel not found",
  "detail": "The daemon is running and retrying by itself. …",
  "conflict": false }
```

`Frames sent` is the daemon's count at the last state event plus the frames received
since. Every state event rebases it, so a reconnect cannot double-count.

On every connection transition the bridge asks for `device_state`, then `status`, and only
then resumes rendering events. The daemon does not replay events, so that snapshot is how
a reconnected (or reopened) window catches up.

### Failure modes, spelled out

| State | What the UI shows |
|---|---|
| Endpoint absent, and the child could not start | **Daemon not running**; the detail names the spawn failure verbatim |
| Endpoint absent, no spawn attempted yet | **Daemon not running**; "…not there (reason). The panel is free" |
| Daemon connected, panel not on the bus | **Panel not found**; the daemon is retrying by itself |
| Daemon connected, device open, vendor service also running | **Two owners**, in a warning line in the session disclosure; the app says this should be impossible and tells the owner to stop the vendor service |
| Request unanswered for 10 s | **The daemon did not answer**; the client drops the link, reconnects, and the request reports `daemon_timeout` |

## What is proven, and what is not

| Area | State |
|---|---|
| Rotation (0/90/180/270) | **Proven on this panel**, 2026-11-18. |
| Display areas (`0x04`, main) | **Proven on this panel**: the layout changed. |
| Display areas (`0x04`, Style Type) | **Verified from the USB capture**, 2026-11-18: index 1 is `00` Normal, `01` Ring, `02` Stopwatch (`re/findings/16-aux-area-layout.md`). It renders only when Main display area is CPU Frequency, and the app removes the row for the other five options. Changing it sends `0x04` plus one telemetry frame. |
| Display areas (`0x04`, auxiliary) | **Verified from the USB capture**, 2026-11-18: the four-byte payload is `[main, style, 0, aux]`, and aux at index 3 is `00` Voltage Mode, `01` System Monitor, `02` Core Data (`re/findings/16-aux-area-layout.md`). It appears after a telemetry frame, so `set_areas` pushes one immediately. |
| Brightness (`0x02` byte 4) | **Proven on this panel.** 20 → 92 visibly dims and brightens. |
| Screen off/on | **Proven**: brightness 0 blanks the panel. `0x02` byte 0 is the idle screen display, not screen power (`re/findings/17-mode-and-idle-screen.md`). |
| Idle screen (`0x02` byte 0) | **Verified from the USB capture**, 2026-11-18: toggled exactly when the vendor control changed, and back (`re/findings/17-mode-and-idle-screen.md`). It does not blank the panel. |
| RGB sync (`0x02` byte 3) | **Verified from the USB capture**, 2026-11-18: toggled six times with the vendor's **Sync with Motherboard RGB Light Effect** checkbox (`re/findings/18-rgb-and-recording.md`). The **direction** was corrected on the panel 2026-11-19: `00` = follow the motherboard, `01` = panel's own effect. |
| Gyro gating (`0x02` byte 1) | **Proven on this panel.** Rotation is ignored while the gyro is on, so rotation always clears it in the same payload. |
| Display mode (`0x03`) | **Verified from the USB capture**, 2026-11-18: `01` Monitor, `02` Media, `03` Recording, with four labelled transitions (`re/findings/17-mode-and-idle-screen.md`). The mode selector sends it; a mode write does **not** erase stored content. |
| Erase / return to dashboard (`0x14`) | **Proven on this panel**, 2026-11-18: it deleted the stored content and the panel showed machine info. It is destructive and has no read-back. |
| Media timing (`0x07`) | High confidence from the capture, **not replayed from this driver**. |
| Recording options (`0x05`) | **Verified from the USB capture**, 2026-11-18: index 0 CPU frequency graph, index 1 CPU temperature graph; both bits moved with the vendor toggles (`re/findings/18-rgb-and-recording.md`). A `0x05` write is followed by one redraw frame. |
| CPU/GPU labels (`0x15`/`0x16`) | Payload shape is proven; the visible effect of a standalone label write has not been observed. |
| Image / GIF upload | **Proven on this panel**, 2026-11-18: the first upload from our own code displayed correctly. Cover cropping is new and not yet on hardware. |
| Telemetry stream (`0x01`) | **Proven on this panel**, 2026-11-18: the style test ran with frames arriving. A single redraw frame is also pushed after `set_areas` and `set_recording`. |
| Daemon stream across a window close | **Fixed in this pass; hardware confirmation still owed.** Closing the window drops the UI's child handle without terminating the process, so the panel should keep updating. Run the pair above to confirm. |

The app never claims a setting is applied because the device accepted the packet. Only
`0x10` reports rotation, and even that byte is advisory. Idle, RGB, brightness, gyro and the
recording options have no read-back at all.

### Why `0x02` is always five bytes

`0x02` carries absolute state, not a delta. The daemon keeps one `DisplayConfig` in its
session and mutates that, so moving the brightness slider preserves the idle screen, RGB
sync, rotation and gyro values last set. Without that, every brightness change would send
the crate defaults and silently reset all four. The UI sees only the resulting
`SettingsState`.

### Screen power is brightness

The 2026-11-18 hardware pass proved `0x02` byte 0 does not blank the panel, while brightness
0 does. `set_screen(false)` therefore writes brightness — 0 for off, the remembered last
non-zero value (70 if unknown) for on — through the same five-byte payload. Byte 0 is the
**idle screen display**, verified separately in `re/findings/17-mode-and-idle-screen.md`
and controllable from the Idle screen switch.

### Stored media still needs erase

`0x03` is the display mode (`01` Monitor, `02` Media, `03` Recording,
`re/findings/17-mode-and-idle-screen.md`), but **stored media dominates the panel while it
exists**. On 2026-11-18, with the panel holding an uploaded image, `0x14` erase alone
returned it to machine info (`re/findings/15-driver-verification.md`). A mode write does
not erase stored content.

**Return to dashboard** and the standalone **Erase stored images** button call the same
`erase_images` request. The section asks for confirmation first, because the action
deletes the stored image or GIF and cannot be undone.

### Style Type and the auxiliary area are style selectors

The `0x04` payload is four bytes, `[main, style, 0, aux]`, from the 2026-11-18 USB
captures (`re/findings/16-aux-area-layout.md`). Index 1 is the primary area's **Style
Type**; index 3 is the **Auxiliary display area** style. Neither is a data source, and
**both only have a visible effect when Main display area is CPU Frequency (`00`)**.

| Index | Field | Options |
|---|---|---|
| 0 | Main display area | `00` CPU Frequency, `01` Time, `02` Pump Speed, `03` CPU FanSpeed, `04` CPU FanSpeed + Pump Speed, `05` Temperature |
| 1 | Style Type | `00` Normal, `01` Ring, `02` Stopwatch |
| 2 | — | always `00`; never observed non-zero, so its meaning is Unknown |
| 3 | Auxiliary display area | `00` Voltage Mode, `01` System Monitor, `02` Core Data |

Index 2 has never been observed non-zero; the daemon always writes `00`. Immediately
after `0x04` the layout redraws but a style does not appear until a telemetry frame
arrives, which is why the auxiliary selector looked dead when the stream was off.
`set_areas` pushes one `0x01` frame immediately and says whether it went out.

## Frontend element ids

The behaviour lives in `ui/js/` and is bound by id only, except for the documented
`data-*` attributes: `data-mode` and `data-modes` on the page and groups, `data-view` and
`data-orient` on the preview, `data-brightness` on the presets, `data-rotation` on the
rotation keys, and `data-pick` on the label selects. The designer can replace `index.html` and the stylesheets wholesale as long
as these ids and attributes stay.

Status bar:

- `daemon-dot`, `daemon-status`
- `handle-dot`, `conn-device-open`
- `service-dot`, `conn-service-running`
- `stream-badge`, `stream-dot`, `stream-word`
  - `stream-word` shows the **counted** frame rate (`1.0 HZ`, `30.1 HZ`), not the word
    "Live", because the panel's numbers change once a second and a working stream
    otherwise looks static. It reads `IDLE` when stopped, `—` before the first
    one-second window closes.
- `btn-log-toggle`, `log-badge`

There is no rate control in the status bar; `set_stream_rate` stays a daemon-side command.

Session disclosure (closed by default; `details#session-details`):

- `daemon-endpoint`, `daemon-child`, `daemon-detail`
- `daemon-conflict` (container, toggled with `hidden`), `daemon-conflict-text`
- `conn-device-present`, `conn-device-info`, `conn-last-error`, `conn-mode`
- `conn-service-warning` (container, toggled with `hidden`), `conn-service-details`,
  `conn-service-output` (verbatim `sc.exe` text)
- `btn-open`, `btn-close`, `btn-stop-service`, `btn-start-service`, `btn-refresh`
- `btn-daemon-retry`
- `btn-daemon-stop` (the explicit stop; warns that it releases the panel and stops telemetry)
- `btn-stream-start`, `btn-stream-stop`
- `stream-source`, `stream-frames`, `stream-failed`
- `stream-achieved-row` (hidden unless achieved differs from requested by more than 5%),
  `stream-achieved`
- `stream-sample`, `stream-transfer`, `stream-max` (milliseconds, one decimal)
- `stream-missing`, `stream-error-note`, `stream-error`
- `startup-enabled`, `startup-minimized` (switches), `startup-error` — write the OS
  start-with-system entry; the entry is read back after every change, so removing it by
  hand is reflected next time the sheet opens
- `conn-error` (inline error)
- `document.documentElement.dataset.daemon` carries the health code

Mode and preview:

- `mode-monitor-btn`, `mode-media-btn`, `mode-recording-btn` (`aria-pressed`), `mode-seg`
  — each click sends `set_mode` (`1`/`2`/`3`) and switches the visible sections; the active
  button changes only after the daemon accepts the write, and a failure appears in
  `app-error`.
- `device-body[data-mode]` and `preview-screen[data-view]` carry the current mode
- `preview-screen`, `preview-style`
- `preview-label`, `preview-value`, `preview-unit`
- `preview-main2`, `preview-label2`, `preview-value2`, `preview-unit2`
- `preview-cell-1`, `-2`, `-3`; `preview-v1..3`, `preview-u1..3`, `preview-k1..3`
- `preview-media`, `panel-media`, `panel-media-empty`
- `rec-freq-block`, `rec-temp-block`, `rec-axis`, `rec-freq`, `rec-temp`,
  `rec-freq-line`, `rec-temp-line`
- `preview-cpu`, `preview-cpu-value`, `preview-gpu`, `preview-gpu-value` (the footer
  shows a sensor only when the panel itself is not drawing it)

Idle screen:

- `idle-screen`, `idle-error`

Brightness:

- `brightness-value`, `brightness-slider`, `brightness-fill`
- `screen-toggle`, `screen-state`, `screen-line`
- `brightness-preset-0`, `-25`, `-50`, `-75`, `-100` (each carries `data-brightness`)
- `brightness-error`

Rotation:

- `rotation-btn-0`, `-1`, `-2`, `-3` (each carries `data-rotation`)
- `rotation-warn`, `conn-rotation-source`, `rotation-error`
- `gyro-toggle`

RGB light:

- `rgb-mode-motherboard`, `rgb-mode-temperature`, `rgb-mode-edge` — one radio group, `rgb-error`
- The three choices are **interlocked**, exactly like the vendor app: one active at a time.
  Motherboard writes `0x02[3] = 00`; the other two write `01` (the panel's own optical
  effect) and differ only in what the panel does in Monitor versus Media mode.

Fan I/O:

- `io-cpu`, `io-pump` (wired: `set_fan_io`, host routing), `fanio-error`

Areas:

- `area-main`, `area-style`, `area-aux`, `areas-error`
- `area-style-row` is the Style Type container, conditionally rendered
- No Send button: any select change writes `0x04` and pushes one redraw frame

Media timing:

- `media-time-0..2` and `media-animation-0..2` (radio inputs)
- `media-error`
- No Send button: any radio change writes `0x07`

Recording:

- `record-freq`, `record-temp`, `recording-error`

Labels:

- `label-cpu`, `label-gpu`, `cpu-counter`, `gpu-counter`, `cpu-count`, `gpu-count`
- `cpu-pick-wrap`, `gpu-pick-wrap`, `label-cpu-pick`, `label-gpu-pick` (hidden until
  `detected_labels` returns names; both selects carry `data-pick="label"`)
- `cpu-label-error`, `gpu-label-error`, `labels-error`
- No Save button: typing commits 500 ms after the last keystroke; blur and Enter commit
  immediately. A label change writes `0x15` and `0x16`.
- `detected_labels` answers `{ "cpu": [...], "gpu": [...] }`. The pick select is a shortcut,
  never a second value. Its options are the disabled prompt `Detected`, every detected
  name in order (the GPU list lists every adapter), then `Custom`.
- A detected pick fills the input, cancels the pending debounce and commits `set_labels`
  at once. `Custom` keeps the current text and focuses the input without committing. In
  both cases the select returns to its prompt, so the input stays the only place the value
  is shown. If detection fails or returns no names, the select stays hidden and typing
  behaves exactly as before.

Image:

- `btn-pick-image`, `image-path`, `media-kind`, `upload-fit`
- The plan builds as soon as a file is chosen and rebuilds when fit changes; there is no
  Preview button. Upload stays explicit and confirmed.
- `upload-plan` (container, toggled with `hidden`)
- `upload-plan-kind`, `upload-plan-size`, `upload-plan-frames`,
  `upload-plan-bytes`, `upload-plan-chunks`, `upload-plan-commands`,
  `upload-plan-content-id`
- `upload-preview-box`, `upload-preview`, `upload-preview-note`
- `upload-plan-details`, `upload-plan-command-list`
- `upload-confirm`, `upload-confirm-copy`, `btn-upload`, `upload-message`, `upload-error`
- `btn-return-dashboard`, `dashboard-confirm-box`, `dashboard-confirm`,
  `btn-dashboard-erase`, `dashboard-state`, `dashboard-report`, `dashboard-error`
- `btn-erase-images`, `erase-message`, `erase-error`

Raw packet:

- `raw-details` (the disclosure), `raw-warning`, `raw-command`, `raw-payload`,
  `raw-pipe`, `btn-raw-dry`, `btn-raw-send`, `raw-packet`, `raw-result`, `raw-reply`,
  `raw-status`, `raw-error`

Log:

- `log-drawer` (container, toggled with `hidden`), `btn-log-toggle`, `btn-log-close`
- `log-live-dot`, `log-live-word`, `log-badge`
- `log-scroll`, `packet-log` (the `<ol>`), `log-empty`, `btn-clear-log`,
  `btn-log-follow`, `log-follow-word`

There are no dead toggles left: the three lighting choices are one interlocked control, and
every other control writes a real command.

Global:

- `app-error` for a missing IPC bridge.

## Layout

```
rust/apps/mystique-ui/
  Cargo.toml                 tauri, mystique-ipc, serde, tokio; nothing device-side
  build.rs                   tauri-build (no CLI required)
  tauri.conf.json            frontendDist = "ui", withGlobalTauri
  capabilities/default.json  core:default + dialog:allow-open
  icons/icon.ico
  src/
    main.rs                  builder, managed state, event bridge, exit hook
    app.rs                   client + launcher + spawn record, boot and retry
    error.rs                 the three client-side codes, on the IPC error shape
    events.rs                notices → Tauri events, resync after a reconnect
    service.rs               sc.exe start/stop for MystiqueService (admin only)
    daemon/
      mod.rs                 DaemonClient handle, LinkState, Notice
      transport.rs           Transport trait, named-pipe / Unix-socket transports
      supervisor.rs          connect, read, match by id, backoff, reconnect
      process.rs             spawn the daemon when absent; kill only our child
      health.rs              the closed set of failure modes, as sentences
      tests.rs               fake transport: matching, events, reconnect, faults
    commands/
      mod.rs                 shared call / call_data helpers
      connection.rs          status, open/close, sc.exe admin, daemon info/retry
      settings.rs            idle, brightness, rotation, gyro, RGB, recording,
                             screen, areas, media, labels
      erase.rs               erase_images, the shared 0x14 request
      raw.rs                 raw_packet, the daemon-built 48-byte request
      telemetry.rs           start/stop stream, telemetry_status projection
      upload.rs              preview and send
  ui/
    index.html               device page: header, session disclosure, groups, raw panel
    fonts/
      inter-normal.woff2     Inter, SIL Open Font License 1.1
    styles/
      fonts.css              the one local @font-face
      base.css               tokens, reset, shared type, lamps, errors
      layout.css             status bar, header, session, columns, groups
      controls.css           keys, switches, slider, rotation, radios, fields, plan, raw
      preview.css            the 640x480 panel, media view, recording graphs
      log.css                the packet log drawer and expanded bytes
    js/
      app.js                 boot, mode selector wire, initial binding
      bridge.js              the only module that imports __TAURI__
      daemon.js              status bar, session disclosure and event routing
      dom.js                 id-based DOM helpers
      errors.js              raw inline errors
      preview.js             staged settings + last frame -> panel preview
      log.js                 packet log, exact bytes, drawer, follow
      raw.js                 hex parsing, dry run, send, log the exact record
      state.js               DTO-to-control mapping and shared state
      sections.js            idle, brightness, rotation, gyro, RGB, recording, areas,
                             media, erase
      labels.js              label typing, detected-name select, immediate pick commit
      telemetry.js           stream controls, counted rate, counters
      image.js               choose, plan, preview, confirm, upload
```

## Tests

The client is tested against a fake transport: a real `tokio::io::duplex` pair with a
scripted daemon. No hardware, no daemon process, and no USB.

```sh
cd rust
cargo test -p mystique-ui
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --workspace
```

Covered by `src/daemon/tests.rs`:

- responses are matched by `id` even when two answers arrive in reverse order, and a
  stray id cannot resolve a waiting request;
- telemetry and state events are forwarded;
- a dropped connection is reopened and answers again;
- an absent daemon is an explicit state, not a hang;
- an unanswered request times out without tearing down the link;
- a request queued before the first connection still completes.

`src/daemon/health.rs` covers all four failure modes as classifications. `src/daemon/process.rs`
covers "already running is not started twice", "a failed spawn is reported", "the exit hook
drops the handle without terminating the daemon", and "an exited child is forgotten so the
next reconnect can start a fresh one". `src/daemon/tests.rs` covers the `shutdown` request
and its answer before the link closes. On Windows, `src/daemon/transport.rs` connects to a
real named pipe (created by the test) so the pipe name and presence probe cannot rot.

## Known limits

- The Rust client is covered end to end against a fake transport. In this pass the page
  was rendered at 1440×900 against a stubbed bridge at every command boundary: idle,
  brightness, rotation, RGB, recording, display areas, the raw panel, every mode switch,
  a plan fills the upload panel, and the log drawer sits beside the content rather than
  over it. The four real `daemon-*` events still need a run against a live daemon with a
  panel.
- Every vendor control is present. The wired set is idle screen, brightness, rotation,
  gyro, RGB sync, labels, display areas, media timing, recording options, upload and erase.
  The three lighting choices are one interlocked radio group (Motherboard / Temperature /
  Image edge), matching the vendor app. No `NO CMD` badge is used.
  Fan I/O is **wired**: `set_fan_io` routes either sensor to either RPM dial on the host.
- The panel preview is a belief: it is built from the staged display-area selects, the
  last `SettingsState` and the last `FrameValues`. Pump and fan RPM come from
  `FrameValues.pump_rpm` and `cpu_fan_rpm`, which the `0x01` frame carries at bytes 21..22
  and 24..25; the three voltage rails are `—` because no byte for them is known. Nothing in
  the preview is presented as a device read. The preview footer shows CPU/GPU temperature
  only when the panel itself is not drawing that value, so no live number appears twice.
  In the CPU FanSpeed + Pump Speed layout the fan number is 52 px and the pump number
  48 px, so neither dial reads as a footnote.
- **The window hides to the tray.** The X button does not exit: `CloseRequested` is
  prevented and the window is hidden, leaving the tray icon, the client and the daemon
  running. The tray menu is **Show control panel / Stop daemon / Quit**, and a left click
  on the icon reopens the window. `Quit` is the only exit and stops nothing; `Stop daemon`
  asks the daemon to shut down over the socket, which blanks the panel and releases the
  device, exactly like the session-sheet button.
- Closing the window **does not stop the daemon**. A daemon spawned by the UI is left
  running with the panel held and the stream up; the next window connects to it. Use
  **Stop daemon** to release the panel and stop telemetry deliberately.
- **Start with system** is a switch in the session sheet. Windows writes
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\MystiqueControl` (no elevation, and
  it appears in Task Manager's Startup tab); Linux writes
  `~/.config/autostart/mystique-control.desktop`. **Minimized to tray** appends
  `--minimized` to that entry, and the window is created hidden so a login launch never
  flashes it. The OS entry is read back after every change, so editing it outside the app
  is reflected the next time the sheet opens.
- **The vendor service is not the daemon.** The status-bar lamp labelled `VENDOR` tracks
  `MystiqueService`; `Stopped` is the healthy state while our daemon owns the device. The
  lamp used to read `SERVICE`, which read like our own service was down.
- **Nothing else starts at boot.** `MystiqueService` is `Automatic`, so after a reboot the
  vendor stack takes the panel unless it is set to `Manual`; the start-with-system toggle
  only starts our window, which then needs the device free.
- The request timeout is 10 seconds. A live `status` read on a wedged panel is the case it
  exists for; the client drops the link and reconnects rather than waiting forever.
- The daemon still implements `set_stream_rate`, but the UI no longer exposes it. The
  status bar counts the rate it is actually achieving, and the session sheet shows the
  achieved rate only when it differs from the requested period by more than 5%; 30 Hz is
  the measured Windows ceiling, not a USB or panel limit.
- Upload chunks are not shown individually; the summary plus the command, header and
  terminator packets are. Telemetry frames are counted in the status bar and are not
  written to the packet log, which would otherwise gain a row every second.
- The raw panel builds and sends any command byte. Unknown commands can change device
  state; `0xA1` starts a panel test pattern sustained by traffic. The panel loads no
  example bytes, and a dry run never touches USB.
- The display-area main label-to-selector order is inferred from the vendor list. Observed
  values were main `0/1/3/5`; unobserved options are still offered because the findings do
  not define a closed list.
- `stop_service` and `start_service` each wait up to five seconds for the service state to
  change. They are the only operations this window still performs directly.
