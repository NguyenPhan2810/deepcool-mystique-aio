# Mystique LCD service + tray — design

Date: 2026-08-06
Status: awaiting user approval

## Problem

`MystiqueUsbTool.exe live 0` drives the DeepCool MYSTIQUE 360 LCD reliably — a measured
12h+ run produced 39,227+ updates, zero errors, zero dropped sequence numbers, every reply
a valid ACK. It works. Two things it does not do:

1. **It has no recovery.** Every exception reaches the top-level catch (`Program.cs:86-91`),
   prints to stderr, and exits. One sensor-bridge restart, USB hiccup, or resume-from-sleep
   silently freezes the LCD until it is relaunched by hand.
2. **It only runs when started manually.** The user wants the LCD live from boot — before
   logon — and a visible indicator that it is running.

## Goals

- LCD shows telemetry from boot, at the login screen, with nobody logged on.
- Survives sensor-bridge restarts, USB replug, sleep/resume, and DeepCool app contention.
- A tray icon after logon showing state, with rotation control.
- Publishable as a public GitHub repo without leaking anything.

## Non-goals

- Firmware flashing. Out of scope, permanently.
- Image/GIF/wallpaper modes. That is what the DeepCool app is for.
- A rich GUI in this pass. The tray is the visible surface; a fuller window comes later.
- Beating the ~1.08 s refresh cadence. That is the sensor bridge's push rate, not a limit
  of ours; changing it means replacing the sensor source and is a separate project.

---

## Architecture

A tray icon cannot exist before logon — the notification area belongs to an interactive
desktop session. Driving the LCD pre-logon therefore requires a Windows Service in
Session 0, and a service cannot show a tray icon. Hence two processes:

```
BOOT  ──► MystiqueService.exe   (Session 0, LocalSystem, SERVICE_AUTO_START)
              │                   owns the WinUSB handle for its entire lifetime
              │                   supervisor: reconnect, backoff, DeepCool yield
              │
              ├── status  ──► named pipe  Mystique.Status.v1   (one-way, out)
              │
LOGON ──► MystiqueTray.exe      (interactive session, WinExe, non-elevated)
              │                   NotifyIcon: state, menu, rotation
              └── commands ──► SCM custom control codes 128-136
```

**The invariant that makes this safe: only MystiqueService ever opens the USB device.**
The tray is a viewer. It never calls `CreateFile` on the device path.

### Why commands do not travel over the pipe

The pipe is **one-way, service → tray**. Commands (rotate, take-over, pause) go through
SCM custom control codes via `ServiceController.ExecuteCommand` / `ServiceBase.OnCustomCommand`.

Rationale: the service runs as LocalSystem. A bidirectional pipe means a LocalSystem process
parsing input from any user session — a local privilege-escalation surface. With SCM control
codes the service parses **zero untrusted bytes**; the only input is an integer in a fixed
range, and Windows already enforces who may send it. Verified: default service SDs grant
`IU` (interactive users) `SERVICE_USER_DEFINED_CONTROL` but *not* `SERVICE_START`/`SERVICE_STOP`,
so a non-elevated tray can send commands without being able to start or stop the service.

Consequence: commands are a small fixed set of parameterless verbs. No file paths, no image
blobs, and explicitly **no "send arbitrary packet"** command — that would be an escalation
primitive.

### Projects

| Project | TFM / output | Packages | Contains |
|---|---|---|---|
| `Mystique.Core.Contracts` | net10.0-windows, lib | 0 | IPC status record types, state enum, command IDs |
| `Mystique.Core.Device` | net10.0-windows, lib | 0 | `Native` P/Invokes, `MystiqueDevice`, `MystiquePackets`, `MachineInfoMode`, `SensorBridgeClient` |
| `MystiqueService` | net10.0-windows, **Exe** | 2 | `ServiceBase`, supervisor, pipe server, logging |
| `MystiqueTray` | net10.0-windows, **WinExe** | 0 | `NotifyIcon`, pipe client, log viewer |
| `MystiqueUsbTool` | net10.0-windows, Exe | 0 | the six diagnostic verbs (unchanged path/name) |

The Contracts/Device split is deliberate: the tray references **Contracts only**, which makes
"the tray cannot open the device" a *compile error* rather than a code-review convention.

`MystiqueService` is `Exe`, not `WinExe` — a service must be a console-subsystem executable.
`MystiqueTray` is `WinExe` so no console flashes at logon.

Core must never set `UseWindowsForms`, `UseWPF`, or a `Microsoft.WindowsDesktop.App`
`FrameworkReference`; it is consumed by a headless service.

Add `Directory.Build.props` at the root and a solution file — neither exists today, and the
two current csproj files already disagree slightly.

### Package cost

Two, both first-party, both on `MystiqueService` only:
`System.ServiceProcess.ServiceController` 10.0.10 and its transitive `System.Diagnostics.EventLog`.

Rejected `Microsoft.Extensions.Hosting.WindowsServices`: ~28 packages and ~27 DLLs for DI,
configuration and `ILogger` this service does not use — and its `WindowsServiceLifetime`
assumes a 30 s shutdown budget when the real one here is 5 s.

Verified in-box on .NET 10, needing **no** package: `NamedPipeServerStreamAcl`, `PipeSecurity`,
`PipeAccessRule`, `PipeAccessRights`, and `Microsoft.Win32.Registry`. (The docs page claiming
`System.IO.Pipes.AccessControl` needs a v5.0.0 package is stale.)

---

## Supervisor

Six states, closed set:

`Starting → Streaming → Degraded → Reconnecting → Waiting → Yielded → Stopped`

Every transition logs one line with the reason and the duration of the prior state.

### Two backoff schedules, not one

**Fast** — fixed 1 s, ±20% jitter, up to 30 attempts. For *expected* conditions: waiting for
the sensor bridge at boot, device arrival after replug, resume from sleep, reopening after
DeepCool exits. These clear in seconds; a textbook exponential curve would leave the panel
dark for 30 s at every boot, a visible regression.

**Slow** — 1, 2, 4, 8, 16, 30 s cap, ±20% jitter. For genuine faults: USB timeouts, stalls,
protocol desync, unknown exceptions.

Cap is 30 s, not minutes: a retry costs one `CreateFile` plus a 2 s pipe connect. Backoff here
protects against log spam and pointless wakeups, nothing else. A five-minute cap would mean the
LCD stays dead for five minutes after the user closes DeepCool — which reads as "broken".

Jitter is not thundering-herd boilerplate. Two deterministic ~1 s retry loops competing for an
*exclusive* handle can lock-step and livelock, each grabbing and losing it in alternation.

**Separate backoff counters per subsystem** (sensor channel / USB device / DeepCool yield).
A chatty sensor bridge must not push USB retries to the cap.

**Reset only after 30 consecutive successes** (~33 s), never on the first. With a flapping
device, resetting on first success turns exponential backoff into a permanent 1 s hammer.

**Never call `Environment.Exit` on a failure path.** Give up only on: a second instance
detected, the same unknown exception 10× in 10 minutes, or explicit user stop — and "give up"
always means a visible Stopped state with an enabled "Retry now", never a silent exit.

Escalate logs on *elapsed failure duration*, not attempt count: WARN at first failure, ERROR at
1 min, ERROR at 5 min, then hourly. A device unplugged for three days produces ~72 lines, not ~8,600.

---

## Failure handling

### Error code taxonomy

| Code | Meaning | Handling |
|---|---|---|
| 2 `FILE_NOT_FOUND` | device not enumerated yet | **normal at boot**, not a fault; fast schedule |
| 5 `ACCESS_DENIED` / 32 `SHARING_VIOLATION` | another owner holds it | treat as *yield*, not error |
| 121 `SEM_TIMEOUT` | pipe transfer timeout | `AbortPipe` + `FlushPipe` **before** retry |
| 31 `GEN_FAILURE` | ambiguous / stall | abort+flush+reset once, then reopen |
| 433 / 1167 | surprise removal | skip pipe recovery, full close + reopen |
| 995 `OPERATION_ABORTED` | cancelled | **swallow if we initiated it**; else device-gone |
| 6 `INVALID_HANDLE` | use-after-free | programming error; guard handle with interlocked swap |

Unknown codes fall into the ambiguous tier. They never crash the service.

### Three traps that would make the "fix" worse than the current crash

1. **`PIPE_TRANSFER_TIMEOUT` defaults to 0 = infinite for bulk endpoints**, and pipe policy is
   state on the handle. A reconnect that reopens the device but forgets to re-apply the timeout
   blocks the worker thread *forever* — no exception, no log, tray showing green, LCD frozen.
   Strictly worse than today's fail-fast. Fold policy application into `Open()` so it is
   indivisible from `CreateFile` + `WinUsb_Initialize`.

2. **After any reopen the device is no longer in Machine Info mode.** Writes are ACKed and
   display nothing. `MachineInfoMode` must be re-run and the `0x10` status byte 5 == `0xFF`
   assertion re-checked before declaring Streaming.

3. **Moving `SetTimeout` out of the per-exchange path removes an accidental liveness probe.**
   Today `SetTimeout` runs on every exchange and throws on a dead device *before* the write,
   which is why current errors are clean. Once policy moves to open-time, the write/read paths
   must explicitly handle the device-gone codes or diagnosability silently regresses.

### Presence detection is currently wrong — replace it

`FindDevicePath` (`Program.cs:94-113`) reads persisted registry state. **22 of 35 `SymbolicName`
entries on this machine belong to absent devices**, so it happily returns a path for hardware
that is not plugged in. At boot this produces a confusing failure: the registry read succeeds,
then `CreateFile` fails with error 2, which reads like "wrong path" but means "not enumerated yet".

Replace with `CM_Get_Device_Interface_List` + `CM_GET_DEVICE_INTERFACE_LIST_PRESENT` filtered to
VID_3633&PID_0009. Presence and path become one atomic, honest query.

### Device arrival

Use `CM_Register_Notification` with `CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE` — windowless,
confirmed by Microsoft docs as the correct choice for code with no window handle. The
`RegisterDeviceNotification` service-status-handle route requires owning `HandlerEx`, which a
`ServiceBase` subclass does not cleanly expose. Never do USB I/O inside the callback; signal the
worker.

### Sleep / resume

Do **not** depend on `SystemEvents.PowerModeChanged`: dotnet/runtime#78162 reports it does not
fire on Windows 11 when the user sleeps manually from the Start menu — the common path — and the
issue is open, externally rooted, and parked on Future.

Design so resume works *without* any notification: the next exchange fails, the supervisor
engages, the LCD returns. Power notification is then a pure optimisation (probe immediately
rather than waiting for the next frame; suppress the "reconnecting" state so an overnight sleep
does not look like a fault in the morning).

Set `RESET_PIPE_ON_RESUME` (policy 0x09) = TRUE at open time on all four endpoints; it defaults
FALSE and exists precisely for devices that do not preserve data-toggle state across suspend.

---

## DeepCool coexistence

**Policy: yield, never fight.**

`HKCU\...\Run` contains `DeepCool = C:\DeepCool\DeepCool.exe -autoRun`, enabled. The user has
been killing it by hand. Per user decision that Run entry is **disabled** (reversible — one
registry value; the two DeepCool *services* stay running and continue to supply sensors).

Evidence disabling it is safe: `DeepCool.exe` has been absent 12+ hours with no ill effect, and
sensor/control logic lives service-side (`SensorBridgeServer.exe` ships `HWiNFO64.dll` and an
80 KB `sensor.json`). The `-autoRun` flag's own handler could not be located — every `autoRun`
match in the 327 MB asar was `electron-updater` internals — so this rests on the empirical
evidence, not on reading its code.

The app can still be launched manually. When it is:

1. Poll `Process.GetProcessesByName("DeepCool")` every 2 s.
2. On appearance: finish the in-flight exchange, then **release the handle pre-emptively** and
   enter `Yielded`. This is the direct antidote to "DeepCool repeatedly tears down a healthy
   handle" — if we are already gone, there is no handle to tear down and no reset war.
3. Make **zero** device calls while it lives.
4. Keep the sensor bridge connected and consuming (discarding values) so takeover is instant.
5. Grey icon, *not* the error icon. No failure counters. Never ERROR level. One INFO line on
   entry, one heartbeat per 15 min, one on exit.
6. On its exit: 3 s settle, reopen, **re-run Machine Info init**, verify `0xFF`, then Streaming.
   Fast schedule — the user just closed it and is watching.
7. Race handling: `ACCESS_DENIED` from `CreateFile` is a synonym for "yield" — re-check for the
   process and transition silently. If DeepCool is *not* present and we still get 5, that is a
   different problem: WARN, slow schedule.

`FILE_SHARE_READ | FILE_SHARE_WRITE` at `Program.cs:31` is preserved and made an explicit
invariant — getting there first with a permissive share mode is what makes takeover work in
our favour.

---

## IPC

**Status pipe** `Mystique.Status.v1`, `PipeDirection.Out`, `PipeOptions.Asynchronous`,
`FirstPipeInstance` on the first, `maxNumberOfServerInstances: 8` (never `-1`).

No `Global\` prefix. Named pipes are **not** session-isolated; `Global\Foo` would create a pipe
literally *named* `Global\Foo`. Verified live: DeepCool's LocalSystem service serves
`deepcool_sensor_data` with no prefix to a non-elevated Session 1 client right now.

**DACL — the highest-probability implementation bug.** A pipe created by LocalSystem with default
security gets LocalSystem's token default DACL: SYSTEM + Administrators only. A non-elevated tray
gets `ACCESS_DENIED` every time, on correct-looking code. Required:

- SYSTEM, Administrators, `NT SERVICE\MystiqueService`: FullControl (only the server needs `CreateNewInstance`)
- `S-1-5-4` INTERACTIVE and `S-1-5-14` REMOTE_INTERACTIVE: `ReadData | ReadAttributes | Synchronize`
- Deny `S-1-5-2` NETWORK, so the pipe is unreachable via `\\host\pipe\`

Do **not** grant Authenticated Users — that includes network logons. Do **not** copy DeepCool's
NULL-DACL pattern (see Security findings).

Payload: NDJSON, `hello` (with `caps[]`) then `status`, heartbeat every 2 s, a `"v"` field plus
version in the pipe name.

**Failure isolation:** per-client task, bounded `DropOldest` channel, `CancellationToken`-bounded
writes. `PipeStream` declares no `WriteTimeout` on .NET 10 — it *throws* — so cancellation is the
only mechanism. The USB loop publishes one immutable snapshot and never touches a pipe; a hung
tray client can never block it.

Tray connects with `TokenImpersonationLevel.Anonymous`.

---

## Service configuration

- **Account: LocalSystem.** No INF, setup-class key, or driver service key applies a custom
  security descriptor, so the device carries the stock USB PDO SD, which grants SYSTEM
  `GENERIC_ALL`. Note the current process works via Everyone/Users/INTERACTIVE with the
  Administrators SID *deny-only*; LocalSystem's token overlaps that only on Everyone, so
  "it works today" does not by itself prove LocalSystem works — the SYSTEM ACE does.
- Harden with `ServiceSidType = unrestricted` (gives `NT SERVICE\MystiqueService` for ACLs;
  removes no group SIDs). **Never `restricted`** — that adds a write-restricted token and is a
  realistic way to break WinUSB access. Trim `RequiredPrivileges` after measuring.
- **`SERVICE_AUTO_START`, not delayed.** Delayed would push LCD illumination out ~2 minutes.
- **No `DependOnService`.** Measured: the Helper Service reports RUNNING at boot+20.0 s but its
  pipe does not exist until boot+24.2 s — a 4.2 s gap dominated by HWiNFO init, consistent across
  8 observed starts. SCM dependencies wait for RUNNING, not for *ready*, so the dependency buys
  nothing the retry loop must provide anyway — while adding a hard failure mode if DeepCool
  renames the service (its key name is literally `Deep Cool Helper Service`, and `nssm.exe` ships
  alongside, so packaging is not fixed).
- SCM recovery actions as a crash backstop, distinct from in-process reconnect.
- `CanShutdown = true` (defaults false, or `SERVICE_CONTROL_SHUTDOWN` never arrives). Opt into
  `SERVICE_CONTROL_PRESHUTDOWN` for a generous window.
- Service vs console mode by explicit `--service` flag in `binPath`, **not**
  `Environment.UserInteractive` (unreliable on modern Windows).
- Absolute `ImagePath`; set working directory explicitly — LocalSystem's CWD is `system32`.
- Deploy to a stable path (`%ProgramFiles%\Mystique\`) via `dotnet publish`, **not** `bin/Release`.
  Windows locks a running image, so building while the service runs from its own build output
  would fail.

Expected first frame on the LCD: **~boot + 25 s**, gated by USB enumeration rather than the
sensor bridge.

### On stop: send nothing

Leave the LCD in Machine Info mode. Abort in-flight transfers, free the handle, exit.

Verified `WaitToKillServiceTimeout` is **5000 ms total**. A mode reset is 13 exchanges with
sleeps; at the current 1500 ms timeout it could exceed 20 s. Attempting it guarantees a force-kill
*mid-sequence*, leaving the device in an indeterminate state — worse than a known one. The panel
is PSU-powered, so at S5 it goes dark regardless, and the next boot re-inits anyway.

On a manual stop, a frozen last frame honestly reads as "nobody is driving me". Resetting to
Image mode would show DeepCool's stale bundled image, which falsely looks like DeepCool took over.

The one place a reset belongs is an explicit, user-initiated **"Release to DeepCool"** command —
not under any SCM deadline.

---

## Logging

Reframed from the initial assumption. 2.44 GiB/year against 171 GB free is **not** a disk problem.
The real defect: 40,000+ identically-shaped lines with **zero timestamps**, and the file only
exists because of shell redirection — `Program.cs` contains no file I/O at all, so under a service
with no console the entire operational log silently evaporates.

- Primary fix is **deleting the per-update line**, not rotation. State transitions + one summary
  per 5 min ≈ 11 MiB/year.
- Rolling file under `%ProgramData%\Mystique`, size-capped, plus Event Log for service
  start/stop/failure.
- **`C:\ProgramData` grants `BUILTIN\Users` create-file and CREATOR OWNER `GENERIC_ALL`.** Create
  the directory with `SetAccessRuleProtection(true, false)` and an explicit DACL, or a standard
  user can pre-create files there and own them.
- In-memory ring buffer the tray can display without reading the file.

---

## Security findings to fix

1. **Unredacted device identifier.** `mystique-live.log:8` contains
   `command 0x12: 55 2E 12 00 41 41 42 42 43 43 44 44` — ASCII `AABBCCDD`, a per-device ID in the
   *response body*. `RedactSerial` only masks the device *path*. Source: `Program.cs:158`
   (`FormatPrefix` hex-dumps the first 12 bytes; the ID sits at bytes 4-11) and `Program.cs:55-57`
   (the `info` verb prints it twice, hex and ASCII). Fires on every `init` and every `live`.
   **Fix:** redact response bodies by default; `info --show-device-id` to opt in. Redaction must be
   the default in the *logging* path, since the service runs unattended.
   Because it is stored as hex byte pairs, a text scan for the identifier does **not** find it —
   which is why the log must be excluded wholesale rather than scrubbed.

2. **Impersonation exposure in the sensor client.** `Program.cs:372-373` and `:399-400` use the
   simple `NamedPipeClientStream` constructor ⇒ `TokenImpersonationLevel.None` ⇒
   `SECURITY_SQOS_PRESENT` unset ⇒ NT default `SecurityImpersonation`. A squatted server could
   impersonate the user's token. One-line fix: pass `TokenImpersonationLevel.Anonymous`. Fix in
   `Mystique.Core.Device`; do not carry the current form forward.

3. **`deepcool_sensor_data` has a NULL DACL** (`O:BAG:SY`, no `D:` component) — a LocalSystem pipe
   every principal can fully control, including `WRITE_DAC`. Recorded as an observed property of
   third-party software and as the anti-pattern our own pipe must not copy. Not ours to fix.

---

## Rotation

`Program.cs:141` already sends the rotation command: `(0x02, [0x01, 0x00, 0x00, 0x01, 0x24])`.
Byte index 2 is rotation — `0x00`/`0x01`/`0x02`/`0x03` = 0°/90°/180°/270°.

Corroborated by qt-deepcool's `PROTOCOL.md` documenting the payload as `01 00 <rot> 01 24`, and by
the divergence itself: their init ships `0x03`, ours `0x00`.

Exposed as a tray menu with the current value checked; persisted in service config; applied on
every init so it survives reconnects.

---

## Publishing

**Include:** `Mystique.Core.*`, `MystiqueService`, `MystiqueTray`, `MystiqueUsbTool`, docs,
`LICENSE`, rewritten `README`.

**Exclude — and physically move out of the tree before `git init`:**

- `deepcool-worker.js` — DeepCool proprietary, extracted from their `app.asar`. Worse than
  assumed: alongside DeepCool's own firmware-update IPC surface, ~1,995 lines are a bundled copy
  of MIT-licensed `electron-log` with its copyright notice stripped by the bundler. Republishing
  would breach **both** DeepCool's copyright and electron-log's attribution clause. A `.gitignore`
  entry is insufficient — `git add -f` exists and history is permanent.
- `mystique-live.log`, `mystique-live-error.log` — contain the unredacted device ID above.
- `bin/`, `obj/` in all projects — verified to contain the username in `.dll`, `.pdb`,
  `project.assets.json`, `FileListAbsolute.txt`, and `GeneratedMSBuildEditorConfig.editorconfig`.
- `asar-*.mjs` — judgment call, not legal necessity. Verified to be the user's own generic code
  with zero DeepCool-specific content. Excluded for *framing*: they contribute nothing to a
  telemetry service and shift the repo's character from "interoperability driver for hardware I
  own" toward "tooling for unpacking a vendor's app". Keep locally; publish separately as
  vendor-neutral `asar-tools` if desired.

No `.git` exists yet — the history starts clean, with nothing to launder.

**License: MIT**, with a real name in the copyright line. Both csproj files have zero
`PackageReference` entries and touch only OS DLLs via P/Invoke, so no inbound license constrains
the choice. MIT matches the ecosystem (qt-deepcool is MIT) and its warranty/liability disclaimers
are wanted for software that writes to hardware. Add a NOTICE making clear the license conveys no
rights in DeepCool's software, firmware, or trademarks, and that no DeepCool code is included.

**Attribution.** No code was copied in either direction; MIT's conditions attach only to copies of
the Software, so nothing is legally required and credit is a courtesy. Do **not** copy their
LICENSE file — that would falsely imply their code is present. Evidence of independence, worth
recording: different language/platform/dependencies entirely; a divergent protocol byte (their
`0x03` vs our `0x00` — someone copying their doc would have copied their byte); a non-ascending
init order that does not match their description; a richer payload map than their doc lists;
offsets expressed in a different frame of reference (packet-relative vs payload-relative); and an
entire subsystem — the `SensorBridgeServer` named-pipe protocol — with no counterpart in their
Linux tool, and as far as can be determined undocumented anywhere public.

Also: no firmware images exist anywhere in the tree (`MYSTIQUE_2.70.bin` is only *referenced* in
`README.md:41`). The README's firmware-safety warning is retained; no firmware capability ships.

---

## Implementation phasing

The work is too large for one pass. Four phases, each independently verifiable, each leaving the
system in a working state.

**Phase 1 — extract Core, no behaviour change.** Split `Program.cs` into
`Mystique.Core.Contracts` / `Mystique.Core.Device`, leave `MystiqueUsbTool` working exactly as it
does today with all six verbs. Add the solution file and `Directory.Build.props`. Success: `live 0`
behaves identically to the current 12 h baseline. Nothing user-visible changes.

**Phase 2 — the service.** `MystiqueService` with the supervisor, presence detection via
`CM_Get_Device_Interface_List`, the error taxonomy, both backoff schedules, DeepCool yield, logging,
and install/uninstall. No tray yet; verify via logs and the LCD itself. Disable DeepCool's Run entry
at this point. Success: cold boot shows telemetry at the login screen, and tests 1-6 pass.

**Phase 3 — the tray.** `MystiqueTray`, the status pipe with its DACL, SCM command codes, rotation
menu. Success: tests 7-8 pass.

**Phase 4 — publish.** Device-ID redaction fix (may be pulled earlier — it is small and independent),
move `deepcool-worker.js` out of tree, `.gitignore`, `LICENSE`, rewritten README, `git init`.

Phases 1 and 2 deliver the reliability win. Phase 3 delivers the visibility the user asked for.
Phase 4 is independent of all of them and could be done at any point after the redaction fix.

## Testing

Unit-testable without hardware: `MystiquePackets` (checksum, offsets, rotation byte), the error
taxonomy classifier, backoff schedule and reset-after-30, the status protocol round-trip.

Requires hardware, done manually against the real device:

1. Cold boot → LCD live at the login screen by ~25 s, before any logon.
2. Restart `Deep Cool Helper Service` mid-stream → Degraded → recovers, no service exit.
3. Unplug/replug the AIO USB → Waiting → reacquires and re-inits.
4. Sleep and resume (both manual Start-menu sleep *and* idle sleep — they differ).
5. Launch DeepCool.exe → Yielded within 2 s, grey icon, no errors logged. Quit it → Streaming
   within ~4 s with the panel correctly re-initialised.
6. Stop the service mid-exchange → clean stop within the 5 s budget, no force-kill.
7. Tray started before the service, and service restarted under a running tray.
8. Rotation applied, then survives a forced reconnect.
9. Confirm the device ID no longer appears in any log after the redaction fix.

Long-run: 72 h+ continuous, compared against the current 12 h baseline of zero errors.

---

## Open items

- **Test whether LocalService can open the device.** If it can, run as LocalService instead and
  the blast radius shrinks substantially (no `SeImpersonatePrivilege`, no `SeTcbPrivilege`). Not
  verifiable read-only; LocalSystem is the verified-safe default until then.
- **`DeepCoolDisplayService.exe` is a latent Session 0 competitor.** Static analysis shows it
  imports `SetupDi*`, `CM_Locate_DevNodeW`, `CreateFileW`, `DeviceIoControl` and runs as
  LocalSystem from boot. Evidence it is *not* taking the LCD today: 25 h running concurrently with
  our process, and zero device-open lines in its logs. Its `swdevice` +
  `deepcooldisplaydriver.dll` imports point at indirect-display creation for screen mirroring, and
  that DLL is not present on disk. **Suspect #1 if a pre-logon conflict ever appears.** Worth
  testing whether it can be set to Manual — it is not the sensor source.
- Whether registering the service requires one-time elevation (expected yes; acceptable).
- `SensorPipeProbe` duplicates the handshake now in Core — recommend folding into a `sensors`
  verb and deleting, to avoid two copies of a reverse-engineered protocol drifting.
