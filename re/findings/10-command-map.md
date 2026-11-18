# Command map — first full sweep of the MYSTIQUE 360

Date: 2026-11-18 (local). Device: `3633:0009`, MYSTIQUE 360, service stopped for the run.
Method: `mystique explore --send --unsafe-all --phase presence` — one 48-byte packet per
command byte, **empty payload**, 750 ms timeout, data pipe for unknown bytes.
Reports: `re/reports/explore-20261117-174551.json` (allowlist), `…-174553.json` (all 256).

## Headline

All 256 command bytes received a reply with `result_code = 0`. The firmware does not reject
unknown commands. The echo is the real "implemented" signal, but **the pipe is part of the
command's identity**: probing the init pipe (finding 11) raises the count from 11 to 18
there, while the data pipe recognises only 2 (plus `0x0A` once payloaded).

> **Corrected 2026-11-18 by finding 11.** The numbers below were the data-pipe answer.
> The union across both pipes is 21 echo-positive bytes, not 11. `0x0F`, `0x11` and
> `0x14` do exist on this 360 (init pipe); the "neutral ACK" for them was a coverage
> artifact.

| Group | Count | Behaviour |
|---|---|---|
| Echoing on init (empty payload) | 18 | `0x02–0x09`, `0x0B–0x0F`, `0x11–0x14`, `0xA1` |
| Echoing on data (empty payload) | 2 | `0x01`, `0x10` |
| Payload-gated on data | 1 | `0x0A` — neutral with an empty payload, echoes with 8 bytes |
| Silent but active | 3 | `0x15`, `0x16`, `0x17`; neutral under two different payloads |
| Neutral ACK only | 232 | Reply `55 2E 00 …` — receipt acknowledged, command not understood on that pipe |

## Implemented commands (echoing)

Data pipe (empty payload):

```
0x01  telemetry        data pipe
0x10  status           data pipe
0x0A  mode switch      data pipe, only with a non-empty payload
```

Init pipe (empty payload), corrected from the init-pipe sweep:

```
0x02  display config   init pipe
0x03  setup step       init pipe
0x04  setup step       init pipe
0x05  setup step       init pipe
0x06  setup step       init pipe
0x07  setup step       init pipe
0x08  setup step       init pipe
0x09  recognised       init pipe, semantics unknown
0x0B  setup step       init pipe
0x0C  recognised       init pipe, semantics unknown
0x0D  recognised       init pipe, semantics unknown
0x0E  recognised       init pipe, semantics unknown
0x0F  begin upload     init pipe
0x11  serial request   init pipe
0x12  device info      init pipe
0x13  recognised       init pipe, semantics unknown
0x14  erase images     init pipe
0xA1  recognised       init pipe, semantics unknown
```

`0x10` returns rotation in byte 4 and mode in byte 5 (`FF` observed = machine-info).
`0x12` returns the 32-hex-char identifier at bytes 4..35, redacted in every report.
`0x11` returns a bare ACK: the reply payload is all zeros, so it yields no serial on
this unit. `0x0A` replies `55 2E 0A 00 …` on the data pipe with either the Machine
Info or the image-mode payload; the reply does not say which mode was selected.

## Silent but active

`0x0A`, `0x15`, `0x16`, `0x17` are all part of the 13-step init sequence that demonstrably
works, yet none echoes its command byte on the **init** pipe. Silence therefore does not
mean "unimplemented".

The probe settled the two candidate explanations differently per command:

- `0x0A` is **payload-gated, but only on the data pipe**. On init it stays silent with its
  8-byte payload (`EA 07 02 02 02 27 21 00`); on data the same packet gets `55 2E 0A 00`.
  An empty payload on data is neutral. A control (`0x02` with its real payload on data)
  stayed neutral, so this is specific to `0x0A`, not a data-pipe framing rule.
- `0x15`, `0x16` and `0x17` stayed silent with both `2D 2D` and `47 50 55` on init.
  "Payload-gated" no longer fits; **write-only** is the better description.

## The image-upload commands do exist here — on the init pipe

`0x0F` (begin upload), `0x14` (erase stored images) and `0x11` (serial request) — the three
commands taken from public prior art for the Mystique 240 — all returned the neutral ACK in
the first sweep. That was a **pipe-coverage error, not a firmware difference**: `--pipe auto`
routes non-allowlisted bytes to the data pipe, so these three were never asked of the init
pipe, where the init sequence lives.

The init-pipe sweep and a targeted probe both show each of them echoing its own command byte
with an **empty** payload, `result_code = 0`. They are real on this 360 and are not
payload-gated for recognition. The data pipe remains neutral for all three, which is why the
first pass could not see them.

What this does and does not establish:

- established: the commands exist and are dispatched on the init pipe;
- not established: that `0x0F` starts an upload, what framing it then expects, that `0x14`
  erases anything, or that `0x11` returns a serial (its reply payload is all zeros).

The prior art's test unit was a **240** and its authors never confirmed the protocol on a
360, so the semantics of these commands on this unit remain `Inferred` even though their
presence is now `Verified`.

## Coverage gap — closed 2026-11-18

Unknown bytes in the first sweep were probed on the **data** pipe only, while the init chain
lives on the **init** pipe. A command implemented only on the init pipe therefore looked
exactly like a no-op.

The third pass (`--pipe init` across all 256 bytes) has now run. It found 18 echoing bytes on
init, including the eight that were invisible to the data-pipe sweep. Treat the remaining
"neutral ACK" entries as "not recognised on that pipe", not "does not exist" — but the
init-side unknown space is now measured, and the result is in `re/findings/11-sweep-results.md`.

## What this means for the driver

- The language is **small**. There is no large hidden command space to chase: the firmware
  distinguishes 21 command bytes by echo across both pipes, and three more only by silence.
- The **pipe is part of the command**. `0x02` is init-only even when payloaded; `0x01` and
  `0x10` are data-only; `0x0A` is silent on init but recognised on data when payloaded. A
  driver that sends the right command byte on the wrong pipe gets a neutral ACK that looks
  like success.
- **Presence is not semantics.** `0x0F`, `0x11` and `0x14` are now confirmed to exist on this
  unit, but no upload has been performed. The prior-art image sequence is still `Inferred`
  end-to-end; treat `mystique image --send` as unvalidated until a real upload is observed.
- The neutral ACK is a trap for tooling: a naive client that checks only for a reply will
  treat every unknown byte as a success. Only the echo, or a verified payload effect, is
  meaningful. `0x00` can never be scored this way: its echo and the neutral ACK are the same
  bytes.
