# Mystique LCD telemetry

Drives the DeepCool Mystique 360 AIO's LCD with live system telemetry, from a
Windows service that starts at boot — the panel is live at the login screen,
before anyone logs on.

No firmware flashing path exists anywhere in this repository.

## Platforms

| | |
|---|---|
| **Windows** | `MystiqueService` (C#) plus the Rust daemon and Tauri control panel in `rust/` |
| **Linux** | the Rust daemon, Tauri control panel and CLI in `rust/`; packaging in `packaging/arch/` |

Same device everywhere: USB `3633:0009`, vendor-specific class, one process owns it at a time.
The protocol record lives in `re/findings/` — that is the part worth reading if you are writing
your own driver for this cooler.

## Installing on Linux

Prebuilt packages are attached to each release:

| System | File |
|---|---|
| Arch | `mystique-<version>-1-x86_64.pkg.tar.zst` |
| Debian / Ubuntu | `deepcool-mystique-aio_<version>_amd64.deb` |
| Fedora | `deepcool-mystique-aio-<version>-1.fc*.x86_64.rpm` |

There is no AUR package: Arch closed new account registration in June 2026. Every package
installs the daemon, the UI and the CLI, the per-user systemd unit, the udev rule and the
desktop entry; the enable steps are in the release notes.

Other distributions build from source — [`docs/INSTALL-linux.md`](docs/INSTALL-linux.md) has
per-distro dependencies, and notes that the daemon and CLI need no GTK/WebKit at all.

## Why this exists

DeepCool's own app drives the panel, but it repeatedly tears down a healthy
WinUSB handle, and it only runs once you are logged in. This replaces the
telemetry path with a service that owns the device handle for its lifetime and
recovers from every failure instead of exiting.

## What it does and does not do

- **Does:** Machine Info mode — CPU temperature, CPU usage, CPU clock, memory
  load, GPU temperature — plus screen rotation, from boot, with automatic
  recovery from sensor-bridge restarts, USB replug, sleep/resume, and DeepCool
  contention.
- **Does not:** images, GIFs, or wallpaper modes. That is what the DeepCool app
  is for, and this yields the device to it on sight.

## Refresh rate

Roughly one update per second. The limit is the DeepCool Sensor Bridge's push
cadence (~1.08 s measured), **not** the device's Full-Speed USB link — a 48-byte
frame once per second is a rounding error against 12 Mbps. Going faster means
replacing the sensor source, which is a separate project.

## Requirements

- Windows 11, .NET 10 SDK to build
- The two DeepCool services must stay **running** — they supply the sensor data:
  - `Deep Cool Helper Service`
  - `Deep Cool Display Service`
- `DeepCool.exe` itself should **not** be running. Only one process can own the
  WinUSB handle. The service yields automatically if you launch it, and
  reacquires when you close it, so this is a preference rather than a rule.

## Install

```powershell
dotnet publish MystiqueService -c Release -o artifacts/publish
```

Then from an **elevated** PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

The service registers as `MystiqueService`, auto-start, running as LocalSystem.
Expect the first frame at roughly boot + 25 s, gated by USB enumeration.

Uninstall:

```powershell
& "$env:ProgramFiles\Mystique\MystiqueService.exe" uninstall
```

Logs, size-capped and rotated:

```
%ProgramData%\Mystique\logs\mystique.log
```

State transitions are logged, plus one summary line per 300 updates. There is
deliberately no per-update line.

## Run in the foreground

Useful for watching it work. Stop the service first, or the two will fight over
the handle:

```powershell
Stop-Service MystiqueService
& "$env:ProgramFiles\Mystique\MystiqueService.exe" --console
```

## Diagnostics

`MystiqueUsbTool` is the hardware probe. It needs the service stopped, since
only one process can hold the device.

```powershell
dotnet run --project MystiqueUsbTool -c Release -- <verb>
```

| Verb | Does |
|---|---|
| `pipes` | list endpoints (default) |
| `status` | report display mode and rotation |
| `info` | device info response |
| `init [rotation]` | run the Machine Info handshake; rotation `0`-`3` = 0/90/180/270 degrees |
| `stream [seconds]` | synthetic telemetry, one update per second |
| `live [updates]` | real telemetry; `0` runs until Ctrl+C |

Add `--show-device-id` to reveal the per-device identifier, which is **redacted
by default**. It is stored in responses as hex byte pairs, so a text search for
it will not find it — which is why log files are excluded from this repository
wholesale rather than scrubbed.

## Layout

| Project | Contains |
|---|---|
| `Mystique.Core.Contracts` | state, telemetry, command ids — no device access |
| `Mystique.Core.Device` | packet format, WinUSB interop, presence detection, sensor bridge, backoff |
| `MystiqueService` | the Windows service, supervisor state machine, logging, installer |
| `MystiqueUsbTool` | diagnostic CLI |
| `Mystique.Tests` | xUnit; filter `Category!=Hardware` to skip anything touching the device |

```powershell
dotnet test Mystique.slnx -c Release --filter "Category!=Hardware"
```

Design notes are in `docs/superpowers/`.

## Firmware safety

The installed DeepCool app's own `app/check-local-fw-update` check returned
`data: false` for its bundled `MYSTIQUE_2.70.bin`. Do not force that image by
filename alone. No firmware image is present in this repository and no firmware
capability ships here; flashing would remain possible only after an image and
target compatibility were independently validated.
