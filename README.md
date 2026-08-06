# Mystique USB audit

Local diagnostics and a live-telemetry workaround for the DeepCool Mystique LCD.
The USB tool contains no firmware flashing path.

## Result

- The device's Full-Speed USB link is not the refresh bottleneck.
- A 90-update synthetic test and a 15-update real-sensor test completed with no
  USB write or response failures.
- DeepCool 1.2.12 repeatedly tears down a healthy WinUSB handle. The replacement
  sender reads the existing DeepCool Sensor Bridge named pipe and sends the
  intended 48-byte Machine Info packet once per sensor sample.

## Run live telemetry

DeepCool.exe must be closed because only one process can own the WinUSB device.
The Deep Cool Display and Helper services remain running to supply sensor data.

```powershell
& 'C:\Users\nguyen\usb-audit\MystiqueUsbTool\bin\Release\net10.0-windows\MystiqueUsbTool.exe' live 0
```

`0` means run until Ctrl+C. A positive value sends that many updates, for
example `live 30` for a 30-update test.

To stop a hidden/background instance:

```powershell
Stop-Process -Name MystiqueUsbTool
```

Current background logs are written to:

- `C:\Users\nguyen\usb-audit\mystique-live.log`
- `C:\Users\nguyen\usb-audit\mystique-live-error.log`

## Firmware safety

The installed DeepCool app's own `app/check-local-fw-update` check returned
`data: false` for its bundled `MYSTIQUE_2.70.bin`. Do not force that image by
filename alone. Firmware flashing remains possible after an image and target
compatibility are independently validated.
