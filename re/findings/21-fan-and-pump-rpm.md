# 21 — Fan and pump RPM: the bytes we were using for the clock

Date: 2026-11-19. Device `3633:0009`. Evidence: `re/captures/usbpcap-fan-mapped.pcap`,
the live vendor sensor bridge, and the owner's eyes on the panel.

## The symptom

With the panel set to `CPU FanSpeed + Pump Speed`, both dials showed **the same number**,
in the 1000–8000 range, and it moved with the CPU clock. It showed a value even with both
Fan I/O selectors on `N/A`.

## The cause

The driver wrote the CPU clock fraction into payload bytes **21..22 and 24..25**:

```rust
let fraction = (cpu_clock_mhz % 1000 / 10) * 100;   // 4300 MHz -> 3000
payload[21..23] = fraction.to_le_bytes();
payload[24..26] = fraction.to_le_bytes();           // "duplicate"
```

Those two slots are not the clock. They are the two RPM dials, so the panel dutifully drew
the clock fraction as fan and pump speed. Both were identical because both were written
from the same value. The "duplicate" was never a duplicate; it was a misread.

The clock needs only **offset 18** (whole GHz) and **offset 20** (`(MHz % 1000) / 10`).

## The evidence

1. **The vendor app changes the dials.** With `MystiqueService` running and `DeepCool.exe`
   open, the owner set both Fan I/O selectors and the two panel numbers swapped with them.
   So the host does send them.
2. **Captured vendor frames carry them.** A 15 s USBPcap window with the mapping set
   (`usbpcap-fan-mapped.pcap`, 14 telemetry frames) shows:
   - `21..22` steady around `0x0D0D` = **3341**
   - `24..25` varying `0x04A1`…`0x072A` = **1185…1834**
3. **The sensor bridge reports exactly those two values.** `SensorPipeProbe` reads:
   ```json
   "Fan list": { "AIO Pump": 3341.0, "CPU": 1326.0 }
   ```
4. **The panel orders the slots.** After the first correction the dials appeared crossed,
   which proved the assignment: the first dial of `CPU FanSpeed + Pump Speed` draws
   **21..22**, the second draws **24..25**.

### Why the capture looked like pump-first

At capture time the owner's two Fan I/O mappings were crossed: the vendor app was feeding
the pump reading into its own CPU slot. So `21..22` held 3341 — the pump value — and the
first correction labelled the slot by the value instead of by the dial. That was wrong.
The numbers are routed by the app; the slot order is fixed by the panel.

## Corrected layout for the bytes in question

| Offset | Width | Meaning |
|---|---|---|
| 18 | 1 | CPU clock, whole GHz |
| 20 | 1 | CPU clock fraction, `(MHz % 1000) / 10`, floor `0x0A` |
| 21–22 | LE16 | **first dial: CPU FAN** |
| 24–25 | LE16 | **second dial: PUMP RPM** |

Everything else is unchanged from `20`'s table: offsets 1..2, 4..5, 7, 10, 13, 16, 19, 23
and 26..38 are zero in every captured vendor frame; offset 8 varies in vendor captures
(values like `0x28`…`0xF7`) and its meaning is still unresolved — our encoder's
`0x05/0x06/0x09` is an inherited guess, not a verified field.

## Consequences

- `TelemetryFrame` gained `pump_rpm` and `cpu_fan_rpm`; the encoder writes them LE16 and no
  longer touches those bytes for the clock.
- `SensorSample` gained both, parsed from the bridge's `Fan list`. `sysinfo` has no fan
  source, so a machine without the vendor helper sends `0`, which the panel draws as
  `0 RPM` — the same thing the vendor app shows with no mapping configured.
- **The routing is a real control.** `set_fan_io` chooses which sensor feeds each dial
  (`cpu`, `pump`, `none`). It sends no setting packet — the panel is told one number per
  dial and nothing else — only one frame to redraw. Defaults are the obvious ones, and
  crossing them reproduces what the vendor app was doing in the capture.
- `re/fixtures/04-telemetry-baseline-0x01.json` is a frame from **our own** C# service
  before this correction. It literally contains two 3000 RPM readings, which is the bug
  frozen in a fixture. The conformance test now says so instead of treating it as truth.
