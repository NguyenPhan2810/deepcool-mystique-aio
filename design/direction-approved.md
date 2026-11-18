# Direction approved

Date: 2026-11-18.

## Shown

Three directions, each a full 1440×900 clickable prototype built from
`design/spec.md` by an independent agent with no sight of the others:

| Direction | Logic | File | Screenshot |
|---|---|---|---|
| A · Warm Editorial | Style wheel #12 (Warm Editorial: cream paper, terracotta, serif display) | `design/directions/a-warm-editorial.html` | `a-warm-editorial.png` |
| B · Instrument Console | Real-world reference (Teenage Engineering OP–1 / TX–6, verified by search) | `design/directions/b-instrument-console.html` | `b-instrument-console.png` |
| C · Rams Functional | Best-designer (Dieter Rams / Braun) | `design/directions/c-rams-functional.html` | `c-rams-functional.png` |

## Superseded — 2026-11-18, after real use

Direction **A** was implemented and used against hardware. It does not survive contact with the
job. Verdict from the owner, in their words:

> "it's too much texts and stupid, UI/UX design is disasterous" — and the purpose is a testing
> harness, not a production app.

The failure is structural, not cosmetic: a document layout with a contents rail, marginalia and
prose is the wrong instrument for a panel you poke at while watching a display. Too much
scrolling, too many words, controls buried in reading flow.

**Direction B (Instrument Console) is now the approved direction.** It was already presented with
a screenshot and never needed a new gate — it was one of the three original options, and this is
a re-selection among them rather than a new design.

What carries from B:

- full-width status bar: device, service, handle state
- a grid of control plates rather than a reading column
- large numeric readouts for live values
- the packet log as a permanent right-hand stream, not an appendix
- one functional accent, monospace for every number

What must be added from the experience with A:

- no explanatory prose in the interface; the packet log is the explanation
- every control reachable without scrolling at 1440×900
- the daemon status line states what is running without claiming ownership

Direction A's implementation is kept only as `design/directions/a-warm-editorial.html` and the
`design/approved-implementation.png` screenshot, as a record of what was tried.


## What carries into implementation

- Cream paper ground, terracotta accent, near-black text, low saturation.
- Newsreader (or the embedded equivalent) for display; Inter for body; no Inter as display.
- **Document structure**: sticky contents rail with numbered sections 01–09; single reading
  column with hairline rules; controls inline in the flow, not floating cards.
- Marginalia column for explanatory notes.
- **Packet log as an appendix** at the end, set as a small monospaced table.
- Numbers set with tabular figures; units subordinate to the value.

## Constraints carried from the spec

- 1440×900; brightness and rotation reachable without scrolling.
- Body ≥14px, labels ≥12px, contrast ≥4.5:1.
- Errors inline in the section that caused them.
- No decorative icons, gradients, emoji, or invented protocol bytes in the log.

## Not carried

- The mockup's illustrative upload figures and the `EBUSY` string: the real app shows a live
  plan from `plan_upload` and real error messages from the backend.
- The mockup's base64-embedded fonts are an artefact of single-file delivery; the app ships
  font files as assets or falls back to a local stack.
