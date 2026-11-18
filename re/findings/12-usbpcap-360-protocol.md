# The 360's real protocol — captured with USBPcap

Date: 2026-11-18. Capture: `re/captures/usbpcap-upload.pcap` (182 MB, 240 s).
Decoded: `re/captures/upload-decoded.json`.
Method: USBPcap filter driver on root hub `\\.\USBPcap1`, vendor app performing a real
upload with a human clicking (synthetic clicks never opened the file picker).

This supersedes the 240-derived protocol in `06-image-upload-protocol.md` for this unit.

## Why our first upload failed

We implemented the 240 exactly, and the 360 rejects it. The decisive difference is how a
frame is transferred.

| | 240 documentation | 360 capture |
|---|---|---|
| Frame transfer | header + JPEG in **one** write | header **alone**, then JPEG in 64-byte chunks |
| `0x02` | `01 00 <rot> 01 24` | `01 01 00 00 46`, later `01 01 00 01 46` |
| `0x03` | `01` | `01`, later `02` |
| `0x04` | `05 00 00 01` | empty |
| `0x07` | `00 02` | empty |
| `0x08` (init) | `00 04` | empty |
| `0x08` (commit) | `01 01` | `01` |
| `0x0A` | `EA 07 02 02 02 27 21 00` | `EA 07 0B 12 0A <counter>` |
| `0x15` | `--` | CPU name, ASCII |
| `0x16` | `--` | GPU name, ASCII |
| `0x17` | `--` | `--` |
| `0x11` | sent | never sent |
| Content id | — | lowercase hex |

## Frame structure — verified

- Every `DCLd` header arrived as **exactly 64 bytes in its own transfer**; not one was
  combined with image data.
- Image data followed as **64-byte transfers**, 8,961 of them on endpoint `0x01`.
- All 14 headers passed our checksum (sum of bytes 0..61, LE16 at 62..63). The header layout
  in `dcl.rs` is correct and unchanged by this finding.
- Static images: `frame_count = 0`, `frame_index = 0`, `delay = 0`.
- GIF: `frame_count = 12`, indices 0..11, `delay = 858` ms per frame.
- Content id: lowercase hex, e.g. `c3bdd15d581d4f6bbc3aeef13d27a8fb`. Same id across all
  frames of one upload, different per upload.

## Observed sequence

```
0x12  info
0x02  01 01 00 00 46
0x03  01
0x04  (empty)
0x07  (empty)
0x08  (empty)
0x05  01 01
0x0B  (empty)
0x06  01
0x15  "Intel Core i9-14900K"
0x16  "NVIDIA GeForce RTX 4070"
0x17  "--"
0x0A  EA 07 0B 12 0A <counter>

0x14                     erase  (seen once, before the first successful upload)
0x0F                     begin
[DCLd header, 64 bytes]
[JPEG, 64-byte chunks]
[dcldfinish, 64 bytes]
0x08 01                  commit
0x0A, 0x10, 0x12         heartbeat / status / info
```

The vendor repeats the init block continuously while the display is active — 130+ times in
240 seconds — so it is not a one-time handshake. Treat the whole block as a keepalive.

## Newly understood: `0x15` and `0x16` are text labels

Earlier testing concluded these do nothing because 3-byte payloads produced no visible
change. The capture shows the vendor sending full ASCII strings: `Intel Core i9-14900K` and
`NVIDIA GeForce RTX 4070`. Payloads up to 39 bytes. This is a real feature, not a
placeholder, and our earlier negative result was an artefact of too-short payloads.

## Still unexplained

- `0x09` — observed twice, payload `01` then `00`. Semantics unknown.
- `0x02` byte 3 — toggles `00`/`01` between sequences. Probably mode or screen state.
- The `0x0A` counter — increments; purpose unknown, likely a heartbeat sequence number.
- Whether `0x14` is needed before *every* upload or only the first. The user's own
  observation ("it worked only after clearing both slots") suggests it matters more than the
  prior art claimed.

## The vendor app is unreliable

Observed by the owner during the capture: an image upload silently did nothing; a GIF upload
worked but displayed the image; clearing both slots and re-uploading the image worked. The
device is fine — the vendor's state machine is not. This is worth remembering before treating
any single vendor failure as a protocol fact.

## Reproduction

```powershell
# capture (elevated; USBPcapCMD auto-elevates and must be killed from an elevated context)
pwsh -File re\tools\usbpcap-window.ps1 -Hub 1 -Seconds 240 -OutFile re\captures\usbpcap-upload.pcap

# decode
py re\tools\decode_usbpcap.py re\captures\usbpcap-upload.pcap --json re\captures\upload-decoded.json
py re\tools\decode_usbpcap_transfers.py re\captures\usbpcap-upload.pcap
py re\tools\summarize_capture.py
py re\tools\extract_upload_sequence.py
```
