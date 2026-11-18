# Fixtures

This directory is intentionally empty of captures.

`re/fixtures/` does not exist in this repository (checked 2026-11-17), and the
brief forbids inventing protocol traffic. Everything asserted in
`tests/test_protocol.py` is derived from the verified constants in
`Mystique.Core.Device/MystiquePackets.cs` and `re/CONTEXT.md` — not from a
fabricated capture. The image/GIF header and sequence tests in
`tests/test_image_upload.py` are derived from the public, hardware-confirmed
record in `dinoyoshi/serial-lcd-panel-client` (MIT), which documents a usbmon
capture of genuine DeepCreative traffic. No bytes were guessed.

If real request/response captures are added under `re/fixtures/`, copy them here
as JSON pairs, for example:

```json
{
  "provenance": "capture | derived-from-reference",
  "command": "0x10",
  "request_hex": "AA 2E 10 ...",
  "response_hex": "55 2E 10 ..."
}
```

Never store an unredacted response body: bytes 4..41 are a per-device
identifier and must be replaced with `..` or zeroed before a fixture is shared.
