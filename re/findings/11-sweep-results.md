# Sweep results — pipe completeness and payload gating

Date: 2026-11-18 (local, UTC+7). Device: `3633:0009`, MYSTIQUE 360.
Service stopped for every run, restarted afterwards. All runs elevated; UAC is
disabled on this machine, so the launch was silent.

Commands run:

```
re\tools\run-explore.ps1 -Full
re\tools\run-probe.ps1 -Batch re\tools\probe-batch-20261118.txt
re\tools\run-probe.ps1 -Batch re\tools\probe-control-20261118.txt
```

Reports: `explore-20261117-175642.json` (allowlist), `…-175644.json` (auto,
all 256), `…-175646.json` (init, all 256), `probe-20261117-175927.json`
(22 payload probes), `probe-20261117-180556.json` (3-probe control).
Run logs: `explore-run-20261118-005638.log`, `probe-run-20261118-005923.log`,
`probe-run-20261118-010552.log`.

## Headline

The device does **not** have one command space. It has two, and the pipe is part
of the command's identity:

| Pipe | Echoing with an empty payload | Commands |
|---|---|---|
| init | 18 | `0x02–0x09`, `0x0B–0x0F`, `0x11–0x14`, `0xA1` |
| data | 2 | `0x01`, `0x10` |
| data, non-empty payload | 1 more | `0x0A` |

`0x0A` is the bridge between the two: silent on init, payload-gated on data.
`0x15`, `0x16`, `0x17` stay silent everywhere and are write-only in practice.

The previous finding's "11 implemented commands" was an artifact of probing
unknown bytes on the data pipe only. **`0x0F`, `0x11` and `0x14` are real on this
360, on the init pipe, with an empty payload.** No payload gate is involved.

## Pass by pass

| Pass | Coverage | Pipe routing | Replies | Echoed | Timeouts | Report |
|---|---|---|---|---|---|---|
| 1 | 15 allowlisted bytes, empty payload | documented pipe | 15/15 | 11 | 0 | `explore-20261117-175642.json` |
| 2 | all 256 bytes, empty payload | allowlist on documented pipe, unknown on data | 256/256 | 11 | 0 | `explore-20261117-175644.json` |
| 3 | all 256 bytes, empty payload | every byte on **init** | 256/256 | 18 | 0 | `explore-20261117-175646.json` |

Every reply arrived in 0–1 ms with `result_code = 0` and `reply_len = 48`.
Pass 3 is the one that closes the coverage gap: it is the only pass that asks the
init pipe about bytes the init sequence does not already use.

Pass 2 reproduces pass 1 exactly, which is the transport sanity check: the same
11 echoes, the same replies, no drift.

`0x00` can never be scored by echo: `55 2E 00` is also the neutral ACK, so
"echoed" and "not understood" are the same bytes. It is reported as neutral.

## Recognised commands, by pipe

`echo` = reply byte 2 equals the command byte, with a non-zero command byte.
Empty payload unless stated.

| Cmd | init | data | data + payload | Reading |
|---|---|---|---|---|
| `0x01` | neutral | **echo 01** | not probed (the service sends this continuously) | telemetry, data only |
| `0x02` | **echo 02** | — | neutral (`01 00 00 01 24`) | display config; init-only even with its real payload |
| `0x03` | **echo 03** | — | — | init step, semantics unknown |
| `0x04` | **echo 04** | — | — | init step, semantics unknown |
| `0x05` | **echo 05** | — | — | init step, semantics unknown |
| `0x06` | **echo 06** | — | — | init step, semantics unknown |
| `0x07` | **echo 07** | — | — | init step, semantics unknown |
| `0x08` | **echo 08** | — | — | init step; `01 01` also echoes |
| `0x09` | **echo 09** | neutral | — | newly recognised; not in the init sequence; semantics unknown |
| `0x0A` | neutral, with payload too | neutral (empty) | **echo 0A** | mode switch; payload-gated on data |
| `0x0B` | **echo 0B** | — | — | init step, semantics unknown |
| `0x0C` | **echo 0C** | neutral | — | newly recognised; semantics unknown |
| `0x0D` | **echo 0D** | neutral | — | newly recognised; semantics unknown |
| `0x0E` | **echo 0E** | neutral | — | newly recognised; semantics unknown |
| `0x0F` | **echo 0F** | neutral | — | begin upload is real on this unit |
| `0x10` | neutral | **echo 10** | — | status, data only; byte 4 rotation, byte 5 mode |
| `0x11` | **echo 11** | neutral | — | serial request is real on this unit, but returns no serial |
| `0x12` | **echo 12** | — | — | device info; 32-hex identifier at 4..35 |
| `0x13` | **echo 13** | neutral | — | newly recognised; semantics unknown |
| `0x14` | **echo 14** | neutral | — | erase images is real on this unit |
| `0x15` | neutral | — | — | write-only; `2D 2D` and `47 50 55` both silent |
| `0x16` | neutral | — | — | write-only; both payloads silent |
| `0x17` | neutral | — | — | write-only; both payloads silent |
| `0xA1` | **echo A1** | neutral | — | newly discovered singleton; semantics unknown |

Union of echo-positive commands across both pipes: **21**. Plus three
silent-but-active (`0x15`, `0x16`, `0x17`) = 24 command bytes with observed
effect or recognition, out of 256.

The eight newly echoed init bytes are `0x09`, `0x0C–0x0F`, `0x13`, `0x14` and
`0xA1`. All return the generic 48-byte `55 2E <cmd> 00 00…` shape: recognition,
no payload data, no result code. Nothing about their arguments is known.

## Probe results

Batch order is safest-first; `0x0A` with the image-mode payload is near the end
and is followed by a Machine Info restore.

| # | Cmd | Payload | Pipe | Echo | Result | Note |
|---|---|---|---|---|---|---|
| 1 | `0x02` | `01 00 00 01 24` | init | **02** | 0 | control; must echo, did |
| 2 | `0x10` | — | data | **10** | 0 | baseline: rotation 0, mode `FF` |
| 3 | `0x0A` | `EA 07 02 02 02 27 21 00` | init | 00 | 0 | machine-info; silent, as in the working init |
| 4 | `0x0A` | same | data | **0A** | 0 | recognised once payloaded |
| 5 | `0x15` | `2D 2D` | init | 00 | 0 | still silent |
| 6 | `0x15` | `47 50 55` | init | 00 | 0 | payload does not unlock an echo |
| 7 | `0x16` | `2D 2D` | init | 00 | 0 | still silent |
| 8 | `0x16` | `47 50 55` | init | 00 | 0 | still silent |
| 9 | `0x17` | `2D 2D` | init | 00 | 0 | still silent |
| 10 | `0x17` | `47 50 55` | init | 00 | 0 | still silent |
| 11 | `0x11` | — | init | **11** | 0 | real command, empty payload accepted |
| 12 | `0x11` | — | data | 00 | 0 | data pipe does not know it |
| 13 | `0x0F` | — | init | **0F** | 0 | real command, empty payload accepted |
| 14 | `0x0F` | — | data | 00 | 0 | data pipe does not know it |
| 15 | `0x14` | — | init | **14** | 0 | real command, empty payload accepted |
| 16 | `0x14` | — | data | 00 | 0 | data pipe does not know it |
| 17 | `0x08` | `01 01` | init | **08** | 0 | prior-art commit payload accepted |
| 18 | `0xA1` | — | init | **A1** | 0 | independent confirmation of the sweep |
| 19 | `0x0A` | `EA 07 08 09 11 3A 09 00` | init | 00 | 0 | image-mode switch; silent |
| 20 | `0x0A` | same | data | **0A** | 0 | recognised on data |
| 21 | `0x0A` | `EA 07 02 02 02 27 21 00` | init | 00 | 0 | restore Machine Info; silent |
| 22 | `0x10` | — | data | **10** | 0 | after everything: rotation 0, mode `FF` |

### The control that makes the `0x0A` result meaningful

`0x02` with its real payload on the data pipe returned the neutral ACK
(`55 2E 00`). So the data pipe does **not** echo every payloaded command. The
`0x0A` echo is specific to `0x0A`, not a framing artifact:

| Cmd | Payload | Pipe | Echo |
|---|---|---|---|
| `0x02` | `01 00 00 01 24` | data | 00 |
| `0x0A` | `EA 07 02 02 02 27 21 00` | data | **0A** |
| `0x10` | — | data | **10** |

The reply to `0x0A` on data is identical for the Machine Info and image-mode
payloads: `55 2E 0A 00 00 …`. The firmware acknowledges the packet but does not
report which mode it selected, so recognition is proven and effect is not.

## Are `0x0F`, `0x14`, `0x11` real on this 360?

**Yes — all three.** Each echoes its own command byte on the init pipe with an
empty payload, and each gets the same 48-byte ACK shape as the init steps. The
earlier "neutral ACK" result was the coverage gap, not the firmware.

What is proven:

- they exist and are dispatched on the init pipe;
- an empty payload is accepted (`result_code = 0`);
- the data pipe does not recognise them.

What is **not** proven:

- that `0x0F` begins an upload, or what framing it then expects;
- that `0x14` erases anything (no observable state delta in two `0x10` reads);
- that `0x11` returns a serial. Its reply payload is all zeros — bytes 4..42
  carry no identifier at all. On this unit, if a serial is wanted, `0x12` is
  still the only command that returns identity data.

So the image pipeline is now plausible on a 360 in the sense that the commands
exist, but the upload itself is still unexercised. `mystique image --send`
should not be treated as validated by this result.

## Device health

**No wedge occurred.** All five operations (three sweep passes, two probe
batches) exited 0. No write timeout, no `pnputil` restart, no reconnect storm
during a run.

One real side effect must be recorded: immediately after the full init-pipe
sweep, the service acquired the device and then failed its first telemetry
write with `Win32 error 22` (`ERROR_BAD_COMMAND`), entering `Reconnecting` for
154 s until it was stopped for the next run:

```
17:56:47.120  device acquired ... rotation 0 degrees
17:56:49.556  Streaming -> Reconnecting: unclassified Win32 error 22
17:56:49.556  WARN device fault: write 0x02 command 0x01 failed: ... (Win32 22)
```

That is **not** a blocked-write wedge — the sweep had already finished and the
harness exited 0 — but the device was left in a state where normal telemetry was
rejected. A service stop/start cleared it; `pnputil` was not needed. The same
sweep minus the init pass (pass 1 only, on the earlier run) did not do this, and
neither targeted probe batch did. Which init-pipe byte causes it is **not
identified**. Practical rule: after a full init-pipe sweep, verify the service
streams; do not assume hand-off is clean.

Current state at the end of this work: see the final section of the reply and
the service log at `C:\ProgramData\Mystique\logs\mystique.log`.

## What changed in the command map

1. The count is not 11. It is 18 on init, 2 on data, plus `0x0A` payload-gated
   on data — 21 distinct echo-positive bytes.
2. `0x0F`, `0x11`, `0x14` exist on this unit. The finding that they did not was
   a pipe-coverage error, not a firmware difference between the 240 and the 360.
3. `0x0A` is not merely "silent but active". It is silent on init and
   payload-gated on data. With an empty payload on data it is indistinguishable
   from an unknown byte.
4. `0x11` returns no serial on this unit. The reply is a bare ACK.
5. New command bytes `0x09`, `0x0C`, `0x0D`, `0x0E`, `0x13`, `0xA1` are
   recognised on init, with no known semantics.
6. `0x15`, `0x16`, `0x17` remain silent under two different non-empty payloads;
   "payload-gated" is no longer a good description of them. Write-only is the
   better fit.

## Unresolved

- **Semantics of `0x09`, `0x0C`, `0x0D`, `0x0E`, `0x13`, `0xA1`.** Recognised,
  no payload data, no state delta measured. Do not name them in code beyond an
  evidence grade of `Unknown`.
- **Whether `0x0A` on data actually switches mode.** It is acknowledged, but the
  reply does not say which payload won. The panel was not visually observed.
- **Whether the image-mode payload took effect at all.** The `0x10` mode byte was
  `FF` before and after, including after the restore. Either the switch is a
  no-op on this unit, or `0x10`'s mode byte is not the indicator (finding 10
  already calls it unreliable).
- **`0x0F` framing.** What comes after "begin upload" on the init pipe is still
  unmeasured. The DCLd terminator and frame size assumptions remain `Inferred`.
- **`0x14` effect.** No observable delta. Erase may be silent by design.
- **How identity is obtained without `0x12`.** `0x11` gives nothing back.
- **The init-sweep side effect.** Which byte leaves telemetry writes failing
  with `Win32 22` is unknown. This is the one result with an operational cost
  and deserves a bisect before the next full init sweep.
- **`0x00`.** Cannot be classified by echo, by construction.
