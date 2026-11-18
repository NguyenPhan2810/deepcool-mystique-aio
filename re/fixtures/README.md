# Conformance fixtures — MYSTIQUE LCD (3633:0009)

Request → expected-response pairs captured from the live device on 2026-11-17.
These are the byte-level expectations a Linux driver should be tested against.
They were produced by `re/tools/make_fixtures.py` from the raw captures in
`re/tools/captures/`; nothing here was hand-typed from memory.

## Redaction rule

The `0x12` reply carries a per-device identifier. It is **never** present in
these files. Where a reply contains it, the fixture gives a template with an
`<identifier>` placeholder and withholds the response checksum (the sum covers
the identifier, so it is device-specific). Where a reply's body is all zero —
which is every reply except `0x12` — the complete 48 bytes, checksum included,
are published.

## Transport facts these fixtures assume

| Fact | Value |
|---|---|
| Request packet | 48 bytes |
| Response packet | 48 bytes (a 64-byte read buffer is supplied; 48 arrive) |
| Init/info endpoints | OUT `0x01`, IN `0x81` |
| Status/telemetry endpoints | OUT `0x02`, IN `0x82` |
| Transfer type | bulk, max packet 64 |
| Request header | `AA 2E <cmd>` |
| Request trailer | `48 49 44 43` (`HIDC`) at 42..45 |
| Request checksum | LE16 sum of bytes 0..45, at 46..47 |
| Response header | `55 2E <cmd-or-00> <result>` |
| Response trailer | `48 49 44 43` at 42..45 |
| Response checksum | LE16 sum of bytes 0..45, at 46..47 |

## Fixture inventory

| File | Command | What it pins down |
|---|---|---|
| `01-info-0x12.json` | `0x12` | Reply length 48; echo `0x12`; result `0x00`; identifier is 32 uppercase hex chars at bytes 4..35; bytes 36..41 zero. Identifier redacted. |
| `02-status-machine-info-0x10.json` | `0x10` | Machine Info mode: byte 4 rotation `0x00`, byte 5 mode `0xFF`; full byte-exact reply. |
| `03-init-sequence.json` | 13-step init | Exact order, payloads, and replies. `0x15/0x16/0x17/0x0A` echo `0x00`; all others echo their command. Post-init status is Machine Info. |
| `04-telemetry-baseline-0x01.json` | `0x01` | A well-formed frame (CPU 36 °C, 9 %, 39 % RAM, GPU 48 °C, 4300 MHz) gets result `0x00`, empty body. |
| `05-telemetry-checksum-mismatch.json` | `0x01` | Checksum +1 → result `0x02`. The device answers, it does not drop silently. |
| `06-telemetry-marker-mismatch.json` | `0x01` | `HIDC` zeroed, checksum valid for that packet → result `0x01`, a distinct error class. |
| `07-checksum-covers-hidc.json` | `0x01` | Checksum computed over 0..41 only → result `0x02`. Proves the device sums 0..45, including `HIDC`. |
| `08-status-checksum-mismatch.json` | `0x10` | Bad request checksum → result `0x02`, **but the reply still carries rotation/mode**. Errors do not discard the command. |
| `09-silent-drop-bad-sync.json` | `0x01` | First byte `0xAB` → no reply at all, only a timeout. Also documents that this wedged the device for ~24 minutes. |
| `10-no-unsolicited-traffic.json` | — | Four reads with no write, both IN endpoints, all time out. Strict request/response. |

## How they were captured

```
dotnet build re/tools/MystiqueCapture/MystiqueCapture.csproj

# one-off captures
MystiqueCapture info        --out re/tools/captures/01-info.json
MystiqueCapture status      --repeat 3 --out re/tools/captures/02-status.json
MystiqueCapture init 0      --out re/tools/captures/03-init.json
MystiqueCapture frame 36 9 39 48 4300 --out re/tools/captures/04-telemetry-baseline.json

# scripted, one field changed per step
MystiqueCapture script re/tools/scripts/01-checksum.json         --out re/tools/captures/05-checksum.json
MystiqueCapture script re/tools/scripts/02-field-validation.json --out re/tools/captures/06-field-validation.json
MystiqueCapture drain --out re/tools/captures/07-drain.json
```

`09` and the post-wedge probes came from `bad-sync`, `poke`, `status` and
`recover` runs after that. They are kept because they are the only evidence of
the failure mode, not because they are safe to repeat.

Regenerate fixtures from the captures with:

```
python re/tools/make_fixtures.py
```

The generator asserts every request checksum, recomputes every zero-body
response checksum, and refuses to include an identifier byte. It also checks
that the `0x10` reply reconstructed from the fields of `02-status.json` is
byte-identical to the captured `0x10` reply in `03-init.json`.

## Limits

- No fixture exists for image/GIF mode (`0x10` mode byte `0x00`); that mode was
  never reached.
- `0x12`'s response checksum cannot be published because it depends on the
  redacted identifier.
- `09-silent-drop-bad-sync.json` is a negative fixture with a real cost: do not
  replay it on hardware.
