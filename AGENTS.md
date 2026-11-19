# Project conventions — Deepcool-AIO-LCD

These are binding for every agent and every human working in this repository.

## File size discipline

No god files. Split by responsibility before a file gets unwieldy.

| Kind | Target | Hard ceiling |
|---|---|---|
| Rust module | ≤ 400 lines | 600 |
| TypeScript / JavaScript | ≤ 500 lines | 800 |
| CSS | ≤ 900 lines | 1500 |
| HTML | ≤ 300 lines | 500 |
| Documentation | ≤ 300 lines | 600 |

When a file approaches its target, split it — do not wait for the ceiling. Split along
responsibility boundaries (one module per concept), never arbitrarily by line count.

A single-file HTML *prototype* is allowed to exceed the HTML limit because the deliverable is
one file; keep it under 1500 lines and split into `html`/`css`/`js` in the same folder if it
grows past that.

## Protocol rules

- **Never invent protocol bytes.** Every constant in `mystique-protocol` carries an evidence
  grade: `Verified` (observed on this 360), `Inferred` (vendor code or prior art), or `Unknown`.
- Unknown semantics stay `Unknown`. Do not give a plausible name to a byte we have not observed.
- The device is the specification. Vendor-app documentation is a hint, not a source of truth —
  a 240-derived protocol already failed on this 360.
- The 48-byte packet, its checksum, and the pipe each command uses are load-bearing. A command
  sent on the wrong pipe looks exactly like a command that does not exist.
- **A byte that advances on every packet is not automatically a counter.** Compare it with
  the wall clock before naming it: seconds roll at 60, minutes at 60, and a "constant
  prefix" may be the capture's own date and hour. `0x0A` was labelled a heartbeat for a day
  because nobody checked that its "counter" incremented once per second
  (`re/findings/22-clock.md`). `re/tools/scan_timeline.py` exists for exactly this: print
  only the changes, then line them up against the time of the capture.

## Device ownership

- Exactly one process may hold the device. `MystiqueService` owns it by default.
- Anything that opens the device directly must stop the service first and restart it after,
  even on failure.
- A wedged pipe is not recovered by restarting the service. It needs a PnP restart
  (`pnputil /restart-device`) or a physical replug.
- Never leave the device in a state that requires a replug without saying so.

## Code quality

- No `unsafe` outside the transport crate.
- No `unwrap` / `expect` / `panic!` in production code; tests may use them.
- Every crate must pass `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, and
  `cargo fmt --check`.
- Errors are typed and shown in place. No swallowed errors, no toast-and-forget.

## Privacy and scope

- **Do not access the camera, microphone, or capture the screen** without explicit, specific
  approval in the current session. An earlier agent did this unasked and it was unacceptable.
- No massaging of process-name checks, no impersonating vendor binaries, no bypassing the
  service's ownership rules.
- Redact the device serial and identifier in anything written to disk. The serial appears in
  logs and reports if you are careless.

## Repository hygiene

- `re/work/` holds extracted vendor material and must never be committed.
- `re/findings/` and `re/reports/` are ours and are tracked.
- Do not modify `Mystique.*`, `docs/`, or `linux/` when working on `rust/`, and vice versa.
- Never commit without being asked.
