# Probe results — `0xA1` is a panel self-test

Date: 2026-11-18. Tools: `re/tools/run-unknown-probe.ps1` (aborted at 12/17) and
`re/tools/run-bisect.ps1` (all 8 candidates). Logs in `re/reports/unknown-probe-*.log` and
`re/reports/bisect-*.log`.

## `0xA1` — full-screen colour cycle, sustained by traffic

Sending `0xA1` with an empty payload puts the panel into a **full-screen colour-cycling test
pattern** — the classic dead-pixel / uniformity test that panels run at manufacture.

### What actually controls it

The pattern animates **while the panel is receiving traffic**. It is not a latched mode:

| Observation | Explanation |
|---|---|
| Stopped when the probe process was killed | traffic stopped |
| Kept going through `0x14`, init, fresh handles, `pnputil` reset | something was still streaming each time |
| Stopped after the service was stopped and nothing wrote for 45 s | genuinely no traffic |
| Panel then showed the vendor logo | idle state, nothing driving |

So there is **no special exit command**, and no power cycle is required. Stop talking to the
panel and it stops; resume normal frames and it may resume. This also invalidates an earlier
claim in this document that the exit was a fresh device handle, and the later claim that it
survived re-enumeration — both tests kept the service streaming throughout.

### What it means

- `0xA1` is a genuine diagnostic, not a display mode.
- It is not dangerous, but it is also **not something to ship as a button**: starting it is easy,
  and the only way to see it stop is to stop all traffic, which also stops telemetry.
- The driver should never send it implicitly. Any explicit exposure needs the "stop traffic to
  stop it" behaviour explained in the UI copy.

## What the probe pass established

| Byte | Result |
|---|---|
| `0x0C` | accepted, no visible effect |
| `0x0D` | accepted, no visible effect |
| `0x0E` | accepted, no visible effect |
| `0x13` | accepted, no visible effect |
| `0xA1` | **colour test pattern** (traffic-sustained) |
| `0x17` | accepted, no visible effect — empty and with ASCII `ABCDEF` |
| `0x08` | accepted, no visible effect outside an upload — value `01` |
| `0x09` | accepted, no visible effect — value `01` |

The last three were re-run isolated, with the service stopped for the whole batch so no traffic
could sustain any state between candidates. All four packets produced nothing observable.

"Accepted, no visible effect" is a real answer, not an absence of one: the firmware acknowledges
these bytes, we sent them in every plausible simple payload, and the panel did not change in any
layout we could put it in. They may require a payload shape we have not guessed, or they may be
write-only registers with no display consequence.

## Recommended handling of `0xA1`

- **Never send it implicitly.** No plan, init or reconnect path may include it.
- If exposed at all, it is a **diagnostic**, labelled as such, with the exit documented
  ("close and reopen the device"). A hidden mode that survives erase is a support hazard.
- If not exposed, record it in the protocol crate as `Unknown`/diagnostic, with a comment that
  it is a test pattern and why we don't use it.

## Still to test

`0x17`, `0x08`, `0x09` — one at a time, with a **close/reopen** between candidates rather than
erase + init. That is the recovery that works.

## Side note

`0x02[3]` was previously unknown. A separate capture resolved it as RGB light effect sync;
see `18-rgb-and-recording.md`.
