# Windows packaging notes

**No INF is required.** The MYSTIQUE exposes the Microsoft OS 1.0 descriptor set
(`USB\MS_COMP_WINUSB`), and Windows already binds it to WinUSB through the generic
compatible ID. `libusb`/`rusb` reach it through WinUSB without a driver replacement.
Do not run Zadig, do not install a libusb-win32 filter, and do not replace the driver:
all three would break the vendor app and are unnecessary.

## Single owner

The device is exclusive in practice. A second open gets `ERROR_ACCESS_DENIED (5)`
even when both sides ask for `FILE_SHARE_READ|FILE_SHARE_WRITE`. Only one of these may
hold it at a time:

- `MystiqueService` (the `Mystique LCD Telemetry` service),
- the vendor app (`C:\DeepCool\DeepCool.exe`),
- this CLI.

`MystiqueService` deliberately releases the device when a process named `DeepCool.exe`
is running, and reacquires when it exits. To use the Rust CLI:

```powershell
Stop-Service MystiqueService    # requires an elevated session
# or let it exist and close the vendor app, then stop the service
```

A non-elevated session cannot stop the service, and the service will race the CLI for
the device. There is no cooperative protocol yet; do not run both.

## Running

```powershell
cargo run -p mystique-cli -- pipes
cargo run -p mystique-cli -- status
cargo run -p mystique-cli -- init --rotation 0
cargo run -p mystique-cli -- live --count 10
```

`explore` writes reports to `re\reports\` relative to the repository root, and
`--emit-constants` rewrites `crates\mystique-protocol\src\commands.rs`. Both are
ordinary file writes; they need no elevation.

## Recovery after a wedge

A packet with a bad sync byte can stop the bulk OUT endpoint from accepting writes.
A service restart does **not** clear it. Restart the PnP device instead:

```powershell
$id = (Get-PnpDevice -InstanceId "USB\VID_3633&PID_0009\*").InstanceId
pnputil /restart-device "$id"
```

or unplug and replug the USB cable. The pump keeps running throughout: the USB rail
powers the logic only.

## Running `mystique-daemon` as a service

**Not implemented in this pass.** The native service wrapper is deliberately absent:
a Windows service must call `StartServiceCtrlDispatcher`, handle `SERVICE_CONTROL_STOP`,
and run without a console. The binary does none of those. `mystique-daemon --service`
therefore exits with code 2 and says so instead of pretending to be a service:

```text
mystique-daemon: --service is not implemented in this build. The binary is a console
application and does not call StartServiceCtrlDispatcher.
```

`sc.exe create` on its own is **not** enough: the SCM would start the process and then
wait forever for a status update it never gets. Use one of these until the wrapper lands.

### Task Scheduler, at logon

Runs as the signed-in user, which is the account the udev-equivalent WinUSB handle is
available to. Create the task once:

```powershell
$exe = "C:\Program Files\mystique\mystique-daemon.exe"
$action = New-ScheduledTaskAction -Execute $exe -Argument "--interval-ms 1000"
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$settings = New-ScheduledTaskSettingsSet -RestartCount 999 `
    -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName "mystique-daemon" -Action $action `
    -Trigger $trigger -Settings $settings
```

`-MultipleInstances IgnoreNew` is a second safety net; the daemon's own single-instance
lock is the real guard and produces the clearer error.

### NSSM, as a real service

NSSM handles the SCM handshake and can send a console stop signal, which the daemon
already handles:

```powershell
nssm install mystique-daemon "C:\Program Files\mystique\mystique-daemon.exe"
nssm set mystique-daemon AppStopMethodConsole 5000
nssm set mystique-daemon Start SERVICE_AUTO_START
nssm start mystique-daemon
```

Use a real SCM wrapper only if it forwards the stop control as `Ctrl+C`/console close;
`TerminateProcess` gives the daemon no chance to write brightness 0 or remove the socket.

### Single owner still applies

The daemon opens `3633:0009` directly. While `MystiqueService` or `DeepCool.exe` owns it,
the open fails with access-denied and the daemon retries. Stop the vendor service first
(`Stop-Service MystiqueService`, elevated) or close the vendor app. Never run the daemon,
the CLI and the UI against the device at the same time.

The control endpoint is `\\.\pipe\mystique-control-v1`. It is local-only; there is no
firewall rule to add and no TCP port to open.

