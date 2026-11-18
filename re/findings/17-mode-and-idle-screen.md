# `0x03` is the display mode after all — and `0x02[0]` is the idle screen

Date: 2026-11-18. Capture: `re/captures/usbpcap-unknown.pcap`. The owner performed three labelled
actions while USBPcap recorded; decoded with `re/tools/diff_blocks.py`.

## `0x03` = display mode

| Time | Payload | Action |
|---|---|---|
| 38.24 s | `01` → `03` | Recording mode |
| 73.34 s | `03` → `01` | back to Monitor |
| 101.54 s | `01` → `02` | Media mode |
| 119.65 s | `02` → `01` | back to Monitor |

Three values, three modes, four labelled transitions. This is now **verified**, not inferred:

```
0x01  Monitor
0x02  Media
0x03  Recording
```

### Why it looked broken earlier

I concluded `0x03` was not the mode because sending `0x03 [01]` plus the handshake did not bring
the panel back from an uploaded GIF. That conclusion was wrong.

Both things are true:

- `0x03` selects the mode.
- **Stored media dominates the display while it exists**, regardless of mode. Erasing (`0x14`)
  is what returns the panel to the dashboard.

So the earlier "erase is the real mechanism" finding is correct *for what you see*, and `0x03`
is correct *for what the device is doing*. They are not competing explanations; they are two
layers. The error was treating them as alternatives.

## `0x02[0]` = idle screen display

| Time | Payload | Action |
|---|---|---|
| 95.92 s | `01 01 00 01 46` → `00 01 00 01 46` | Idle Screen Display Settings: off |
| 99.34 s | `00 01 00 01 46` → `01 01 00 01 46` | back on |

So byte 0 is the **idle screen** toggle — what the panel does when it would otherwise sleep —
not screen power, and not brightness. Brightness 0 remains the way to blank the panel on demand.

## Media-mode bytes seen while in Media

While in Media mode the vendor also moved:

- `0x07` byte 1: `02 → 01 → 00` — the image-switch animation cycling
- `0x08` byte 0: `01 ↔ 00` — toggling; purpose still unclear (likely playback state or index)

## Still unknown

- `0x02[3]` — never seen change under a labelled action.
- `0x04[2]` — never non-zero.
- `0x08` semantics, beyond "toggles around media playback".

## Consequences for the app

- Re-introduce a **Display mode** control with three values, marked verified.
- Keep **Return to dashboard** (erase) and explain both: mode decides behaviour, stored content
  decides what is visible.
- Rename `0x02` byte 0 from `unknown_byte0` to **idle screen**, and expose it as a toggle.
- Do not claim `0x02[0]` blanks the screen — that is brightness 0.

## Driver status

`0x03` is now writable from our own code: `mystique-protocol` has
`DisplayTarget { Monitor = 01, Media = 02, Recording = 03 }` and `set_mode`, and the daemon
exposes `{"cmd":"set_mode","args":{"mode":3}}`.

Sent to the real panel on 2026-11-18 and accepted:

```
AA 2E 03 03 00 … 00 48 49 44 43 F6 01     0x03 payload 03 -> result success
AA 2E 03 01 00 … 00 48 49 44 43 F4 01     0x03 payload 01 -> result success
```

`mode: 9` is rejected at the daemon with code `range` and never reaches the wire.

Nothing here changes the stored-media rule: a mode write does not erase an uploaded image.
