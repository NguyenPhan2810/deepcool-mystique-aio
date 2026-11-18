# Protocol mapping plan — the device is the specification

Goal: a complete, evidence-graded map of the MYSTIQUE's command language, sufficient to
implement a driver with no reference to DeepCool's software.

Explicit non-goal: reverse engineering the vendor app. Its Mystique implementation is V8
bytecode and is not decodable with available tooling. It is also unnecessary — every fact we
need can be obtained from the device over USB.

## What "complete" means

A command is **mapped** when we know, from observation:

| Property | How we learn it |
|---|---|
| It exists | It replies, rather than being ignored |
| It is accepted | Response byte 3 == `0x00` |
| Its reply shape | Length, echoed command byte, which offsets vary |
| Whether it changes device state | `0x10` status delta before/after, or visible panel change |
| Its payload sensitivity | Reply changes across a set of probe payloads |

A command is **not** mapped if we only know it appears in the init sequence.

## Method

### Phase A — presence sweep

For each command byte `0x00..0xFF`: send a 48-byte packet with an **empty payload**, read the
reply with a short timeout, record:

```
{ cmd, replied, echo, result_code, reply_len, reply_hex (device id redacted), elapsed_ms }
```

The firmware does not range-check or cross-check payload fields (proven on the real device),
so an empty payload is the lowest-risk probe. Some commands will legitimately time out; a
timeout is a result, not a failure, and must be recorded with its duration.

### Phase B — state delta

For each command that replied with success, bracket it with `0x10` status reads and record the
before/after pair. This catches rotation-style changes that are readable in status even when
we cannot see the panel.

### Phase C — payload sensitivity

For commands that show a state delta, probe payloads systematically: zero, `0xFF`,
ascending `00 01 02 …`, and a short ASCII marker. Record whether the reply changes and whether
the status delta changes. Commands whose reply is invariant under all payloads are either
one-shot triggers or write-only — both are useful to know.

### Phase D — sequence replay

Replay the known-good init sequence and the C122-style upload preamble, recording every reply.
This validates the sequences we already intend to ship and gives regression fixtures.

## Safety rules

1. **Dry-run by default.** The explorer prints the plan and sends nothing unless told.
2. **Allowlist for real sends.** `0x10`, `0x12`, `0x01`, `0x02`, plus the init set, are safe
   to send by default. Everything else requires `--unsafe-all`.
3. **Empty payload only** for unknown commands. Never send a non-empty payload to a command
   whose semantics are unknown.
4. **No firmware path.** If a command produces a long-running or unusually large response, stop
   the sweep and record it rather than retrying. Firmware transfer is out of scope permanently.
5. **Wedge protocol.** A blocked write means the pipe is stuck. Recovery is a PnP device
   restart (`pnputil /restart-device`) or a replug — a service restart alone is **not** enough,
   learned the hard way. Stop the sweep after the first wedge; do not continue into a second.
6. **One owner.** The service must be stopped for the duration. Restart it afterwards.

## Evidence grades

Every constant that lands in `mystique-protocol` carries one:

- `Verified` — observed on this 360, in `re/reports/explore-*.json`
- `Inferred` — decompiled from vendor code (`C122`/`L086`) or documented by prior art, not
  observed here
- `Unknown` — named, present, semantics unresolved

Anything `Unknown` must not be given plausible-looking behaviour in code.

## Deliverable

1. `re/reports/explore-<stamp>.json` — raw sweep output, diffable across runs.
2. A generated table in `rust/crates/mystique-protocol/src/commands.rs` where each command is a
   constant with its grade and a comment pointing at the evidence.
3. A short `re/findings/09-command-map.md` summarising: how many bytes replied, which clusters
   exist, which produce state changes, and what remains genuinely opaque.

## What this will not tell us

- The **visual** effect of a command, without a camera or a human looking at the panel. This is
  a real limitation, not a shortcut. Semantics that only exist on screen (brightness levels,
  label text) cannot be confirmed by protocol alone.
- Whether a command is destructive. Absence of a bad reaction in one probe is not proof of
  safety; this is why the gate exists.

Where a visual claim is needed, the correct move is to ask the owner to look at the panel at a
specific moment — one glance, not a UI walkthrough — or to omit the claim.
