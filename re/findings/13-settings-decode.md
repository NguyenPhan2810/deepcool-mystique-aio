# Settings decode — brightness, rotation, gyro, display areas, media options

Date: 2026-11-18. Capture: `re/captures/usbpcap-settings.pcap` (548 MB, ~9 min).
Method: USBPcap on root hub 1 while the owner changed one control at a time in the vendor
app. Decoded with `re/tools/diff_blocks.py`, which diffs consecutive config blocks and prints
only what changed, with timestamps.

The vendor app re-sends its whole config block continuously, so a settings change appears as a
single-field diff. That makes the mapping unambiguous **where the diff is unique**, and
ambiguous where several controls moved in the same window — flagged below.

## `0x02` — display configuration

Five meaningful bytes; the rest are zero padding.

```
index  0      1        2        3        4
      const  gyro?    rotation  ?        brightness
```

| Index | Meaning | Observed values | Confidence |
|---|---|---|---|
| 0 | screen/display enable | `01` normally, `00` at 304.61s → back to `01` at 314.68s | Medium — plausible but not confirmed by a labelled action |
| 1 | **gyroscopes allowed** | `01` = allowed, `00` = off | **High** — toggled twice, matches the UI control exactly |
| 2 | **rotation** | `00`=0°, `01`=90°, `02`=180°, `03`=270° | **High** — cycled 0→1→2→3→0 at 61–71s and again at 248–268s |
| 3 | mode/state flag | `01` mostly; `00`/`01`/`02`/`03` appear in other sequences | Low — see note |
| 4 | **brightness** | `0x46` (70) default; `0x15` (21) and `0x00` (0) observed | **High** — three distinct values, matching slider drags |

**Behavioural rule discovered by the owner:** rotation only takes effect when gyroscopes are
**off**. The vendor UI enforces this. A driver must do the same, or rotation commands will be
silently ignored while the gyro is enabled.

Byte 3 is the one part that does not resolve cleanly. It moved `00`↔`01` at 91s and 304s, and
appeared as `01`/`02`/`03` in the upload-related sequences. Candidates: media type, recording
state, or an "image vs GIF" selector. **Not resolved — do not encode a guess.**

## `0x03` — a two-valued selector

Observed `01` and `02`, toggling at 56.7s, 106.9s, 151.3s, 374.9s, 385.1s. Every switch
coincided with a mode change in the app (Monitor ↔ Media). Most likely the **display mode**:
`01` = machine info, `02` = media. Confidence: **medium** — consistent with the timing, but no
single isolated action proves it.

## `0x04` — two display-area selectors packed into one payload

```
index  0                1                 2.. rest
      main area index   auxiliary index   zero
```

| Index | Meaning | Observed values | Confidence |
|---|---|---|---|
| 0 | **Main Display Area** | `00`, `01`, `03`, `05` cycled at 189–193s; `03` at 329s; back to `00` at 338s | **High** — matches the dropdown exactly |
| 1 | **Auxiliary Display Area** | `02` at 344.79s, back to `00` at 353.64s | **High** |
| 2 | unknown | `01` at 360.31s, `00` at 368.27s | Low — possibly Style Type |

"Style Type" was changed around 360s and produced a change at index 2, but only one sample.
Treat as **tentative**.

## `0x07` — media switch timing

```
index  0               1
      switch time      animation
```

| Value | Meaning (from the UI) |
|---|---|
| time `00`/`01`/`02` | 3 s / 5 s / 7 s |
| animation `00`/`01`/`02` | Split Display / Scroll / Fade In/Out |

Sequence observed: `00 01` → `01 01` → `02 01` → `02 02` → `00 00` → `00 01`, matching the
owner stepping through Image Switch Time and then Animation. Confidence: **high**.

## `0x15`, `0x16`, `0x17` — text labels

ASCII strings, zero-padded, max 39 bytes.

| Cmd | Content observed | Meaning |
|---|---|---|
| `0x15` | `Intel Core i9-14900K` | CPU name |
| `0x16` | `NVIDIA GeForce RTX 4070` | GPU name |
| `0x17` | `--` | third label, purpose unknown |

The earlier finding that these "do nothing" was an artefact of testing 3-byte payloads. They
carry full names and are re-sent with every config block.

## `0x08` — commit / state

Toggles `00` ↔ `01`. Appears as `01` immediately around an upload and as the commit after the
frame data. Outside an upload it also toggles during mode changes, so it is not exclusively a
commit. Confidence: **medium**.

## `0x0F` — begin upload

Appears when an upload starts, absent otherwise. Confirmed three times in the upload capture.

## `0x09` — unresolved

Only two occurrences in the upload capture, payload `01` then `00`. Not reproduced in this
settings capture. **Unknown.**

## What is now implementable

- brightness set (0–100 in `0x02[4]`)
- rotation set (`0x02[2]`), gated on gyro off (`0x02[1] = 0`)
- gyro toggle
- main and auxiliary display area selectors (`0x04`)
- media switch time and animation (`0x07`)
- CPU and GPU name labels (`0x15`, `0x16`)
- screen enable (`0x02[0]`) — medium confidence

## What remains unknown

- `0x02[3]` — media type / recording state
- `0x09` — both payloads seen, purpose unknown
- `0x04[2]` — probably Style Type, one sample only
- `0x17`'s purpose
- whether `0x14` is required before every upload

## Replay verification — partially done

The driver was pointed at the panel and each decoded setting was sent from our own
encoder (`re/tools/run-verify.ps1`, log `re/reports/verify-*.log`). All six commands
were accepted (exit 0); what the panel actually did:

| Step | Sent | Observed |
|---|---|---|
| 1 | brightness 20 | not observed — the owner could not tell in a timed script |
| 2 | brightness 92 | not observed, same reason |
| 3 | rotate 1 (90°) | **confirmed** — panel rotated |
| 4 | rotate 0 | **confirmed** — panel returned upright |
| 5 | areas main 5 / aux 2 | **confirmed** — bottom field became Temperature, aux became Core Data |
| 6 | screen off / on | not observed, same reason |

So `0x02` byte 2 (rotation) and `0x04` (display areas) are now **proven end to end
from our own code**, not merely decoded. Brightness and screen-enable remain decoded
but unproven; a timed script is the wrong instrument for them, which is why an
interactive control UI is being built (`rust/apps/mystique-ui`).

## Honest limits of this decode

- One unit, one firmware, one session.
- Semantics are attributed by **action order**, not by isolated single-variable tests. Where
  two controls moved within the same block window, the attribution is marked low confidence.
- Nothing here was verified by replaying the command from our own driver yet. That is the next
  step and it is cheap: send the exact bytes, watch the panel.

## Reproduction

```powershell
pwsh -File re\tools\usbpcap-window.ps1 -Hub 1 -Seconds 600 -OutFile re\captures\usbpcap-settings.pcap
py re\tools\diff_blocks.py re\captures\usbpcap-settings.pcap
```
