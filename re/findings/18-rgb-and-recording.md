# `0x02[3]` is RGB sync, `0x05` is the recording options

Date: 2026-11-18. Capture: `re/captures/usbpcap-remaining.pcap`. Owner exercised the vendor
controls that had never been labelled. Decoded with `re/tools/diff_blocks.py`.

## `0x02[3]` — RGB light effect sync

Byte 3 of the display config toggled `01 ↔ 00` six times in a row at 47–52 s, while the owner
was clicking **Sync with Motherboard RGB Light Effect** on and off. Nothing else in that window
moved.

```
01 01 00 01 46   sync on
01 01 00 00 46   sync off
```

Confidence: high on correlation, and it is the only control that was being touched during that
window. It is a flag the LCD firmware uses to defer the ring colour to the motherboard, not an
LCD drawing parameter.

### Direction, corrected 2026-11-19

The labels above were an assumption: the capture correlated the byte with the checkbox but did
not pin which click produced which value. On the panel the effect is the opposite of what those
labels say:

```
00  follow the motherboard   (sync ON)
01  panel's own effect       (sync OFF)
```

Verified by toggling on hardware: at `01` the ring ignored the motherboard, at `00` it followed
it. The vendor init block carries `00`, so the vendor app starts synced — which also explains
why the driver's old default (`00`, written while its setting claimed "off") looked like an
inverted checkbox.

The protocol encoder now writes `00` for "synced", and its default is synced so the captured
init block stays byte-exact.

Note: the RGB **temperature warning** toggle produced no LCD command at all, so that one is
handled elsewhere and is out of scope for a display driver.

## `0x05` — recording mode options

`0x05` is in the init chain as `[01 01]`, which we had marked "unknown setup step". In Recording
mode the vendor rewrites it continuously, and the owner's two recording toggles moved it:

| Time | Payload | Meaning |
|---|---|---|
| 60.81 s | `01 00` → `01 01` | second option on |
| 61.95 s | `01 01` → `00 01` | first option off |
| 64.22 s | `00 01` → `01 00` | first on, second off |
| 65.34 s | `01 00` → `01 01` | second on |
| 66.46 s | `01 01` → `01 00` | second off |
| 67.61 s | `01 00` → `01 01` | second on |

So `0x05` is `[first option, second option]` for the on-panel recording graph. The reproduction
UI describes these as **CPU frequency** and **CPU temperature**. Which index is which is not yet
pinned — the click order was not recorded — but the two bits are certainly those two options.

`0x03` also moved `01 → 03` at 59.68 s, confirming Recording mode as mode value `3`.

## Fan I/O produced nothing

**CPU Fan I/O** and **Pump Fan I/O** produced no LCD command, exactly as expected: they map fan
headers to the pump/fan controller, not to the display. A display driver has no business with
them, and their absence from the capture is evidence, not a gap.

## What this leaves

Known now:

```
0x02[0] idle screen     0x02[1] gyro      0x02[2] rotation
0x02[3] RGB sync        0x02[4] brightness
0x03    mode            0x04[0] main      0x04[1] style
0x04[3] aux             0x05[0..1] recording options
```

Still unknown:

- `0x04[2]` — never non-zero.
- `0x06`, `0x0B` — init steps, effect never observed.
- `0x08` — toggles around media and upload; semantics unclear.
- `0x09` — payloads `01` and `00` seen, meaning unknown.
- `0x0C`, `0x0D`, `0x0E`, `0x13` — recognised by the sweep, never sent by the vendor.
- `0x0A` — payload `EA 07 0B 12 0A <ctr>`; only the counter is understood.
- `0x17` — a third label slot, purpose unknown.
- `0xA1` — recognised, unknown.

The vendor app cannot reach these, so they can only be settled by sending them ourselves and
watching the panel.
