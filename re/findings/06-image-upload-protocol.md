# Image and GIF upload protocol — implementation contract

Status: **recovered from public prior art, hardware-confirmed by that project, not yet
confirmed on this unit.** Derived from
[`dinoyoshi/serial-lcd-panel-client`](https://github.com/dinoyoshi/serial-lcd-panel-client)
(MIT), which obtained it from `usbmon` captures of genuine DeepCool traffic and validated it
against a physical Mystique 240. Verify on this machine before trusting it for a 360.

This document exists because the repo's Windows implementation deliberately omits images.
Everything here is additive to the verified telemetry protocol in `re/CONTEXT.md`.

## Why it matters

Without this, a Linux driver is functionally a downgrade from the vendor app: telemetry only,
no custom artwork. This is the whole reason the effort exists.

## Corrections to the shared brief

Two statements in `re/CONTEXT.md` are now known to be wrong or incomplete:

1. **"Image/GIF upload is completely undocumented"** — it is documented, below and upstream.
2. **"After the init sequence, `0x10` must report mode `0xFF` or the device did not enter
   machine-info mode"** — a real capture showed `0x00` while the panel was visibly live in
   machine-info mode. The flag is unreliable. **A Linux port must not gate init on it**;
   log it and gate on the handshake completing without a transfer error instead.

Also answered: the panel is **640x480 landscape**, vendor-stated; uploads are sent as
**480x640 portrait** JPEGs, which is the hardware-confirmed working size.

## Commands added

| Cmd | Name | Payload |
|---|---|---|
| `0x0F` | begin image upload | empty |
| `0x14` | erase stored images | empty. **Mandatory before a second upload** — without it the next upload silently no-ops |
| `0x11` | serial request | empty. Image-mode handshake only |

Two mode-switch payloads for `0x0A`:

```
machine info:  EA 07 02 02 02 27 21 00
image mode:    EA 07 08 09 11 3A 09 00
```

## The 64-byte frame header

Per frame, a `DCLd` header is followed **in the same USB transfer** by the raw JPEG bytes.

```
off     size  field
0       4     "DCLd"
4       1     0x02
5       4     JPEG length, u32 LE
9       4     sum of JPEG bytes mod 65536, u32 LE (upper 2 bytes always zero)
13      1     total frame count
14      1     unused
15      2     per-frame delay in ms, u16 LE
17      1     0-based frame index
18      2     unused
20      32    content/session id, 32 ASCII hex chars, constant across one upload
52      10    unused
62      2     sum of the header's own first 62 bytes, u16 LE
```

The content id is unvalidated by firmware — any stable 32-hex-char value works. An MD5 of
the concatenated JPEG bytes is a reasonable choice.

## The transfer sequence

```
1. 0x0A  image-mode switch
2. 0x10  status request
3. 0x01  display update, payload 00 00 00 00 00 00 54 00 1E   (see caveat)
4. 0x12  device info
5. 0x11  serial request
6. init sequence steps 2..13 (config through display labels)
7. 0x14  erase stored images
8. 0x0F  begin image upload
9. per frame: [64-byte DCLd header || JPEG bytes]  as ONE transfer
10. "dcldfinish" zero-padded to 64 bytes
11. 0x08  with [0x01, 0x01]   -- the commit, NOT init's [0x00, 0x04]
12. repeat steps 1..6 entirely
```

Step 3's payload is reproduced verbatim from the capture and looks like a counter/heartbeat
rather than something load-bearing. Step 12 is reportedly **necessary**; skipping it uploads
without error and displays nothing.

## Three silent-failure traps

These all produce a successful-looking transfer and a screen that never changes. They are
the highest-risk part of the whole effort.

1. **Header and JPEG in separate writes.** The header is exactly 64 bytes, one
   `wMaxPacketSize`, so libusb inserts a zero-length packet between the two writes. The
   device processes the upload and shows nothing. Build one buffer.
2. **JPEG without a JFIF APP0 segment.** Genuine DeepCreative uploads always carry one
   immediately after SOI. A decoder that rejects it fails silently, with no protocol error.
3. **Optimized Huffman tables.** The panel's decoder accepts only standard/fixed tables.
   A default-optimized encoder fails the same silent way.

In Python, Pillow satisfies 2 and 3 by default — it emits JFIF APP0 and uses standard tables
unless `optimize=True`. Assert both properties in a unit test rather than assuming:

```python
assert data[:4] == b'\xff\xd8\xff\xe0'      # SOI + APP0
assert b'JFIF\x00' in data[:16]
```

## Known unresolved limitation

**After an image upload, the device cannot reliably be returned to machine-info mode.** The
upstream project tested several hypotheses — sending `0x14` first, a third `0x0A` heartbeat
payload with an incrementing counter, and a second mode-switch — none reproducible. Their one
untested lead is a roughly 4-second traffic gap coinciding with fresh `GET_DESCRIPTOR` control
requests, suggesting the vendor app **closes and reopens the USB handle** during the
transition.

Practical consequence for the Linux driver: treat mode switching as a one-way door per
connection, and consider making "switch mode" a full close/reopen cycle rather than a command
burst. This is worth testing early, because it determines whether the driver can offer both
features in one daemon or needs a restart between them.

## Confidence levels

| Claim | Confidence |
|---|---|
| Framing, headers, checksums | High — matches our own verified protocol exactly |
| Sequence order | High — reproduced from captures |
| JFIF / Huffman requirements | High — failing cases were tested deliberately |
| `0x14` required before re-upload | High — three consecutive uploads succeeded with it |
| Panel is 640x480 | Medium-high — vendor spec page, not measured |
| Upload size 480x640 | Medium — one project, one 240 unit, portrait only |
| Behaviour on a **360** | **Untested anywhere.** This unit is a 360. |
| Return to machine-info mode | Low — not solved by anyone |

The 240-vs-360 gap is the one that matters most here: every hardware confirmation above came
from a Mystique 240, while this machine has a 360. The protocol is very likely identical, but
that is an assumption until tested.
