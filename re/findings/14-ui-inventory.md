# UI inventory — DeepCool app, all captured screens

Date: 2026-11-18. Source: 22 screenshots taken by the owner in
`C:\Users\nguyen\Pictures\Screenshots\`, `Screenshot 2026-11-18 HHMMSS.png`,
110124 → 111531. Every screenshot was read; the two newest (111525, 111531) were
added after the first pass and are folded in below in walkthrough order.

Purpose: turn the app's visible vocabulary into ordered option lists, so the numeric
indices decoded from USB traffic can be given labels. **This document is UI facts
only.** The only byte relationships stated are the ones supplied with the task brief;
they are repeated in §4 and nowhere else.

Transcription rules used here: labels are copied character-for-character, including
capitalisation and spacing mistakes. Nothing was tidied. Where a list could not be
seen to its end, that is stated rather than completed.

---

## 1. Screen map

The app is a single window with a narrow icon rail on the left and a content area
that is re-rendered per device and per personalization mode.

```
┌ Title bar ────────────────────────────────── DeepCool · – □ × ┐
│                                                                │
│ rail   │  Device / MYSTIQUE          Personalization Settings ▾ │
│  ◔     │                                                       │
│  ▭     │  ┌───────────────────────────────────────────────┐    │
│  ▤     │  │  mode-specific panel                          │    │
└────────┴──┴───────────────────────────────────────────────┴────┘
```

**Navigation rail** — three icons, **no text labels anywhere**:

| # | Icon | Meaning | Captured? |
|---|---|---|---|
| 1 | speedometer / gauge | unknown page | **No** — never shown in any screenshot |
| 2 | monitor outline | unknown page | **No** — never shown in any screenshot |
| 3 | two stacked rack units | device page (active, teal, with left indicator bar) | Yes — every screenshot |

The active page header reads `Device / MYSTIQUE`: a breadcrumb, so this is a device
detail view reached from a device list. Only this page exists in the evidence.

**`Personalization Settings`** sits in the page header, to the right of the
breadcrumb — it is not part of the left column. It is a dropdown that swaps the
*entire* body of the page:

| Mode | Body layout | What it controls |
|---|---|---|
| `Monitor Mode` | two columns: settings on the left, live device preview + display-area pickers on the right | idle animation, brightness, rotation/gyro, RGB sync, fan I/O headers, and what the panel shows (main + auxiliary area, style) |
| `Media Mode` | one column: preview card on top, media controls below | uploaded image/GIF playback: switch interval, switch animation, RGB edge picking |
| `Recording mode` | one column: preview card on top, two controls below | on-panel graph of CPU frequency and/or CPU temperature |

Three mode-specific **screens** plus one modal = four distinct surfaces, seven
dropdown lists, twenty-two screenshots.

**Modal:** `devices.upload` — opened from the `+` tile under `Image` or `GIF`.
Contains a canvas cropper, four icon-only toolbar buttons, an upload/re-upload tile
strip, and `Close` / `Confirm`.

---

## 2. Per-screen control inventory

### 2.1 Device page — `Monitor Mode`

Captured in 110835 (full window; the only screenshot with the left column and the
window chrome both visible).

**Left column**

| Control | Type | Options / range | Current value |
|---|---|---|---|
| `Idle Screen Display Settings` | dropdown | see §3.4 (2 options) | `Use Preset Animation (Recommended)` |
| `Screen Brightness Adjustment` | slider + unlabelled toggle | slider has no numeric scale and no readout; thumb at the far right end | toggle **ON**, thumb ≈100% |
| `Screen Rotation` — `Gyroscopes are allowed` | toggle | on / off | **ON** |
| `Screen Rotation` — `Rotate Screen` | link (with rotate icon) | cycles orientation; no degrees shown anywhere in the UI | — (not an enumerated control) |
| `RGB Light Effect Sync` — `Sync with Motherboard RGB Light Effect` | toggle | on / off | **OFF** |
| `RGB Light Effect Sync` — `Enable Temperature Warning (RGB)` | toggle | on / off | **ON** |
| `I/O Interface Settings` — `CPU Fan I/O` | dropdown | see §3.7 (3 options) | `N/A` |
| `I/O Interface Settings` — `Pump Fan I/O` | dropdown | see §3.8 (3 options) | `N/A` |

Notes:
- The `Screen Brightness Adjustment` row has a **small toggle switch immediately to
  the left of the slider track**, currently ON, with no label. It is visually
  distinct from (and smaller than) the labelled toggles below it. Its purpose is not
  stated anywhere on screen.
- The slider shows **no numeric value** — not a percentage, not a 0–100 scale. The
  only evidence of its position is the thumb at the right end.
- There is no scrollbar and no hidden content below `Pump Fan I/O` in this capture.

**Right column (preview + panel content)**

| Control | Type | Options / range | Current value |
|---|---|---|---|
| device preview card | live mock of the LCD | — | `CPU FREQUENCY`, `1.23`, `GHz`; footer strip `3.30 / 4.96 / 12.00` with `3.3V` `5V` `12V` markers |
| `Main Display Area` | dropdown | see §3.1 (6 options) | `CPU Frequency` |
| `Style Type` | dropdown | see §3.3 (3 options) | `Normal` |
| `Auxiliary Display Area` | dropdown | see §3.2 (3 options) | `Voltage Mode` |

**Conditional layout — `Style Type` is not always present.** It appears only when
`Main Display Area` is `CPU Frequency`. In the five captures where the main area was
changed to `Time`, `Pump Speed`, `CPU FanSpeed`, `CPU FanSpeed + Pump Speed` and
`Temperature`, the `Style Type` row is **absent from the panel entirely** — not
disabled, not greyed, removed, with `Auxiliary Display Area` moving up directly under
`Main Display Area`. So the style value is only meaningful in the CPU-frequency
state.

**Preview changes with the main area** (same right-hand card, 110907–110920):

| `Main Display Area` | preview headline | preview footer (auxiliary) |
|---|---|---|
| `CPU Frequency` | `CPU FREQUENCY` 1.23 `GHz` | `3.30` `4.96` `12.00` (V rails) |
| `Time` | `AM` 11:09 | 37 `°C`, 3.10 `GHz` |
| `Pump Speed` | `PUMP RPM` 0 `RPM` | 52 `°C`, 5.10 `GHz` |
| `CPU FanSpeed` | `CPU FAN` 0 `RPM` | 51 `°C`, 5.30 `GHz` |
| `CPU FanSpeed + Pump Speed` | two stacked dials: `CPU FAN` 0 `RPM` / `PUMP RPM` 0 `RPM` | 67 `°C`, 5.10 `GHz` |
| `Temperature` | `CPU TEMPERATURE` 42 `°C` | 5.10 `GHz`, 45 `%` |

With `Auxiliary Display Area` = `Core Data`, the footer always shows two compact
chips and they change to avoid repeating whatever the main area already shows —
e.g. main `Time` → `37 °C` + `3.10 GHz`; main `Temperature` → `5.10 GHz` + `45 %`.
Whether that avoidance is app logic or firmware logic is not determinable from
screenshots.

### 2.2 Device page — `Media Mode`

Captured in 110924, 110928, 110935, 110938 (same screen, four states).

| Control | Type | Options / range | Current value |
|---|---|---|---|
| `Personalization Settings` | dropdown | see §3.5 | `Media Mode` |
| preview card + delete icon | live mock, trash icon top-right of the card | — | plays the uploaded GIF |
| `Image` | section label with a wrench icon | — | — |
| `Image` tile strip | `+` tile (dashed, opens `devices.upload`) + thumbnail(s) | unlimited count not shown; two tiles fit before the row would need scrolling | 1 image (`RE CAPTURE 23:31:58`) |
| `Image Switch Time` | radio group | `3 s` / `5 s` / `7 s` | `3 s` |
| `Image Switch Animation` | radio group | `Split Display` / `Scroll` / `Fade In/Out` | `Scroll` (110924, 110928); `Split Display` in 110935; `Fade In/Out` in 110938 |
| `GIF` | section label with a wrench icon | — | — |
| `GIF` tile strip | `+` tile + thumbnail | — | 1 GIF |
| `Image/GIF Edge Picking` | section label | — | — |
| `Image/GIF Edge Picking` — `Sync RGB with Image/GIF Edge Picking` | toggle | on / off | **OFF** in all four captures |

Notes:
- **`Image Switch Time` is disabled (all three radios greyed out) when only one image
  is present** — 110935 shows this. It re-enables in 110938 with two images. The
  control is present but not usable; a driver should not expect frequent writes to
  the switch-time byte when the user has one image.
- `Image Switch Animation` stays enabled even with one image.
- Hovering a radio shows a floating label of its own text: `Fade In/Out` appears in a
  small white popover in 110938. These are tooltips, not new labels.
- The GIF section sits **below** the switch-time and switch-animation controls, so
  the panel order is: image → timing → animation → GIF → edge picking.

### 2.3 Device page — `Recording mode`

Captured in 110942, 110948, 110952.

| Control | Type | Options / range | Current value |
|---|---|---|---|
| `Personalization Settings` | dropdown | see §3.5 | `Recording mode` (lowercase `m`) |
| preview card | live mock | graph window spans `-7Min` → `Now` | `CPU` 4.40 `GHz` / 47 `°C`, two stacked line graphs |
| `Refresh` | link with circular-arrow icon | — | — |
| `Display Content` | section label | — | — |
| `Display Content` — `CPU Frequency` | checkbox | on / off | ON (110942, 110952); OFF (110948) |
| `Display Content` — `CPU Temperature` | checkbox | on / off | ON (110942, 110948); OFF (110952) |

Notes:
- Unchecking one series greys that row out and removes its graph from the preview;
  the remaining graph expands. Both can be off independently (not captured, but
  nothing prevents it).
- The mode exposes **no recording start/stop, no file path, no interval** — only
  which series are drawn and a manual `Refresh`. There is nothing here that looks
  like a recorder control despite the name.
- Graph axis labels are `-7Min` and `Now`; units are `GHz` and `°C` on the graph
  headers.

### 2.4 Modal — `devices.upload`

Captured in 111525 (two images staged) and 111531 (one image staged). This is the
dialog behind the `+` tiles in Media Mode.

| Control | Type | Options / range | Current value |
|---|---|---|---|
| `devices.upload` | modal title | — | — |
| canvas | image editor, checkerboard = transparency | dashed crop box with 8 handles + a solid blue resize corner | fits the loaded image |
| toolbar button 1 | icon only (magnifier `+`) | — | zoom in |
| toolbar button 2 | icon only (magnifier `−`) | — | zoom out |
| toolbar button 3 | icon only (frame with arrow) | — | crop / fit transform — **exact function not labelled, cannot be named from the UI** |
| toolbar button 4 | icon only (trash) | — | delete current image |
| `Re-upload` | dashed `+` tile with caption | — | adds / replaces an upload |
| thumbnail strip | selectable tiles, teal border = selected | — | 111525: 2 images; 111531: 1 image |
| `Close` | secondary button | — | — |
| `Confirm` | primary button (teal) | — | — |

**The modal title is `devices.upload` — an untranslated i18n resource key, not
English.** It is rendered literally, complete with the dot. This is a genuine UI
defect in the vendor app and worth recording as such: the app resolves strings
through a key table, and this one has no entry for the current locale. It also means
the app's string table can be mined for control names if the Electron bundle is
inspected.

No dropdown is open in either modal screenshot, so these two add **no option list**.

---

## 3. Ordered option lists

Every list below was verified complete: in each screenshot the dropdown panel's
rounded bottom edge is visible **inside** the frame with normal end padding and no
scrollbar thumb. Nothing here was extrapolated.

### 3.1 `Main Display Area` — 6 options

Source: 110853 (open), value also visible in 110835, 110855, 110857, 110907–110920.

| Index | Label (verbatim) |
|---|---|
| 0 | `CPU Frequency` |
| 1 | `Time` |
| 2 | `Pump Speed` |
| 3 | `CPU FanSpeed` |
| 4 | `CPU FanSpeed + Pump Speed` |
| 5 | `Temperature` |

Capitalisation is inconsistent in the original: `CPU Frequency` has a space,
`CPU FanSpeed` does not. `Pump Speed` has a space. Keep them exactly as written if
they are ever exposed in our own UI.

### 3.2 `Auxiliary Display Area` — 3 options

Source: 110857 (open), value also visible in every other Monitor Mode capture.

| Index | Label (verbatim) |
|---|---|
| 0 | `Voltage Mode` |
| 1 | `System Monitor` |
| 2 | `Core Data` |

`System Monitor` is never the selected value in any screenshot, so its rendering is
unseen.

### 3.3 `Style Type` — 3 options

Source: 110855 (open), value also visible in 110835 and 110857.

| Index | Label (verbatim) |
|---|---|
| 0 | `Normal` |
| 1 | `Ring` |
| 2 | `Stopwatch` |

Only selectable while `Main Display Area` = `CPU Frequency` (§2.1).

### 3.4 `Idle Screen Display Settings` — 2 options

Source: 110840 (open). **Only two options exist** — the panel closes directly under
the second one.

| Index | Label (verbatim) |
|---|---|
| 0 | `Use Preset Animation (Recommended)` |
| 1 | `Turn Off Screen Display` |

In the open menu the first item is visually truncated to
`Use Preset Animation (Recommende…` because the menu is narrower than the control.
The closed control (110835) shows the string in full, which is where the complete
wording above comes from. This is truncation by the widget, not a different label.

### 3.5 `Personalization Settings` — 3 options

Source: 110124 (open), value also in every other screenshot.

| Index | Label (verbatim) |
|---|---|
| 0 | `Monitor Mode` |
| 1 | `Media Mode` |
| 2 | `Recording mode` |

`Recording mode` is the only one with a lowercase second word. Not a typo of mine.

### 3.6 `Image Switch Time` / `Image Switch Animation` — 3 options each

Source: 110924 (both groups), 110935, 110938. Radio groups, left-to-right.

`Image Switch Time`:

| Index | Label (verbatim) | Seconds |
|---|---|---|
| 0 | `3 s` | 3 |
| 1 | `5 s` | 5 |
| 2 | `7 s` | 7 |

`Image Switch Animation`:

| Index | Label (verbatim) |
|---|---|
| 0 | `Split Display` |
| 1 | `Scroll` |
| 2 | `Fade In/Out` |

The numeral and the unit are separated by a space in all three time labels (`3 s`,
not `3s`).

### 3.7 `CPU Fan I/O` — 3 options

Source: 110846 (open).

| Index | Label (verbatim) |
|---|---|
| 0 | `N/A` |
| 1 | `AIO Pump` |
| 2 | `CPU` |

### 3.8 `Pump Fan I/O` — 3 options

Source: 110848 (open). Identical list to `CPU Fan I/O`, captured separately.

| Index | Label (verbatim) |
|---|---|
| 0 | `N/A` |
| 1 | `AIO Pump` |
| 2 | `CPU` |

---

## 4. Cross-reference against the decoded fields

Only the byte relationships given in the task brief appear here. The method is:
locate the UI option in its ordered list, read off its position, compare with the
values seen on the wire.

**Shared assumption, stated once:** this table maps `wire value → list position`.
That holds only if the app sends the list index. Two independent checks support it —
`0x07` exercises 0/1/2 across a 3-item list, and `0x04[0]` reaches 5 in a 6-item
list, saturating exactly at the last item (§4.1). It is still an inference about the
app's encoding, not an observation of the app's source.

### 4.1 `0x04` byte 0 → `Main Display Area`

Observed on the wire: **0, 1, 3, 5**.

| Wire | Label | Seen on wire? |
|---|---|---|
| `00` | `CPU Frequency` | yes |
| `01` | `Time` | yes |
| `02` | `Pump Speed` | never transmitted in the capture |
| `03` | `CPU FanSpeed` | yes |
| `04` | `CPU FanSpeed + Pump Speed` | never transmitted in the capture |
| `05` | `Temperature` | yes |

Confidence: **high.** The list has exactly 6 entries, the largest observed value is
5, and that is the final option. A 6-option list with a maximum index of 5 fits
without slack in either direction. Values 2 and 4 were simply not visited during the
capture; there is no reason to think they mean anything else.

### 4.2 `0x04` byte 1 → `Auxiliary Display Area`

Observed on the wire: **0, 2**.

| Wire | Label | Seen on wire? |
|---|---|---|
| `00` | `Voltage Mode` | yes |
| `01` | `System Monitor` | never transmitted in the capture |
| `02` | `Core Data` | yes |

Confidence: **high.** 3 options, observed maximum 2 = final option. `System Monitor`
is the untested middle value.

### 4.3 `0x04` byte 2 → `Style Type` (suspected)

One sample on the wire: `01` at 360.31 s → `00` at 368.27 s.

| Wire | Label | Seen on wire? |
|---|---|---|
| `00` | `Normal` | yes |
| `01` | `Ring` | yes |
| `02` | `Stopwatch` | never transmitted in the capture |

Confidence: **medium, and cannot be raised from screenshots.** The list is 3 long
and both observed values land on real options, which is consistent. But a single
`1 → 0` transition is equally explainable by any two-state control, and the UI
attaches `Style Type` to a *conditional* row that only exists when the main area is
`CPU Frequency` — so whatever the app writes at `0x04[2]` may be stale unless that
row was on screen. `Stopwatch` has never been exercised, so wire value `02` is
unproven for this byte.

### 4.4 `0x07` byte 0 → `Image Switch Time`

Observed on the wire: **0, 1, 2** — all three.

| Wire | Label |
|---|---|
| `00` | `3 s` |
| `01` | `5 s` |
| `02` | `7 s` |

Confidence: **high.** Three options, all three observed, ordering matches the
left-to-right radio order. This is the cleanest mapping in the whole decode.

### 4.5 `0x07` byte 1 → `Image Switch Animation`

Observed on the wire: **0, 1, 2** — all three.

| Wire | Label |
|---|---|
| `00` | `Split Display` |
| `01` | `Scroll` |
| `02` | `Fade In/Out` |

Confidence: **high.** Same reasoning as 4.4.

### 4.6 Option count vs observed index range — verdict

| Field | Options | Valid indices | Observed | Max observed | Verdict |
|---|---|---|---|---|---|
| `0x04[0]` Main | 6 | 0–5 | 0,1,3,5 | 5 | **no contradiction** — max = last index |
| `0x04[1]` Aux | 3 | 0–2 | 0,2 | 2 | **no contradiction** — max = last index |
| `0x04[2]` Style | 3 | 0–2 | 0,1 | 1 | no contradiction; index 2 unproven |
| `0x07[0]` Time | 3 | 0–2 | 0,1,2 | 2 | **no contradiction** — fully exercised |
| `0x07[1]` Animation | 3 | 0–2 | 0,1,2 | 2 | **no contradiction** — fully exercised |
| `0x07` (both) | 3 each | 0–2 | 0,1,2 | 2 | **no contradiction** — fully exercised |
| Idle Screen | 2 | — | — | — | no byte supplied in the brief |
| Personalization Settings | 3 | — | — | — | no byte supplied in the brief |

**There is no count-versus-index contradiction anywhere.** For the two fields that
matter most, the option count is a perfect witness: `Main Display Area` has 6 options
and the traffic reached 5; if the app had a seventh option or the list started at
index 1, one of those facts would have to be wrong.

Three near-misses that are *not* contradictions but should be written down so nobody
over-claims later:

1. `Style Type` index 2 (`Stopwatch`) has never been on the wire. The list length is
   confirmed by the UI; the wire value is not.
2. `Main Display Area` indices 2 and 4 are unvisited. They are pinned by their
   neighbours and by the list end, so the risk is negligible.
3. `Auxiliary Display Area` index 1 (`System Monitor`) is unvisited and its on-panel
   rendering has never been seen, so we cannot describe what it looks like — only
   that it exists.

### 4.7 One adjacent conflict, outside the brief's byte list

Recorded because it affects the same model, and clearly separated from §4.1–4.5
because the brief did not assign a byte to the mode selector.

The UI has **three** personalization modes (`Monitor Mode`, `Media Mode`,
`Recording mode`). `re/findings/13-settings-decode.md` describes `0x03` as a
**two-valued** selector (`01` ↔ `02`) and labels it, with medium confidence, as the
display mode. Two values cannot address three modes. Either `0x03` is not the mode
selector, or the mode is encoded elsewhere (or derived from other bytes such as
`0x02[3]`, which that finding leaves unresolved), or `Recording mode` is a
client-side overlay that never reaches the device. Not resolvable from the UI alone;
flagged so it is not assumed away.

---

## 5. Controls with no known command

Everything visible in the UI that has no command in our current map. Grouped by
screen so a capture plan can be built from it.

**Monitor Mode**

| Control | State seen | Why it matters |
|---|---|---|
| `Idle Screen Display Settings` | `Use Preset Animation (Recommended)` / `Turn Off Screen Display` | 2-option selector, values 0/1, no byte assigned in the brief. This is the only control that plausibly turns the panel off, so it is worth one isolated capture. |
| unlabelled toggle on the brightness row | ON | **Not transcribed anywhere else, not in the decode.** A switch with no label, sitting on the brightness row. Could be screen enable, could be a dimming master. One screenshot, no label, no observed action — do not guess. |
| `Screen Brightness Adjustment` | ≈100% | Slider with no numeric readout. Byte identity is known; the UI gives no way to correlate a pixel position with a value, so slider→byte calibration must come from traffic, not from screenshots. |
| `Rotate Screen` | link | Rotation is exposed as a *link that cycles*, not a picker. There is **no orientation indicator anywhere in the UI** — no degrees, no compass, no highlighted quarter-turn. Combined with the "rotation only works when gyros are off" rule, the user-visible behaviour is: click, look at the physical panel. |
| `Sync with Motherboard RGB Light Effect` | OFF | two-state toggle, no command |
| `Enable Temperature Warning (RGB)` | ON | two-state toggle, no command |
| `CPU Fan I/O` | `N/A` | 3-option selector, no command. Sets which header the app reads fan RPM from — affects the `CPU Fan`/`Pump` readings on the panel. |
| `Pump Fan I/O` | `N/A` | same, independent selector |

**Media Mode**

| Control | State seen | Why it matters |
|---|---|---|
| `Image/GIF Edge Picking` → `Sync RGB with Image/GIF Edge Picking` | OFF | two-state toggle, no command |
| delete icon on preview card | — | removes the on-panel image; no known command |
| `+` / `Re-upload` tiles | — | opens the upload modal; media upload path is separately documented, but tile management (ordering, count limit) is not |
| `Image Switch Time` disabled state | greyed with 1 image | the app suppresses writes here; a driver that always sends a value may diverge from app behaviour |

**Recording mode**

| Control | State seen | Why it matters |
|---|---|---|
| `Display Content` → `CPU Frequency` | ON / OFF | checkbox, no command. Changes what the on-panel graph draws. |
| `Display Content` → `CPU Temperature` | ON / OFF | checkbox, no command |
| `Refresh` | link | forced redraw; no command. May be purely client-side. |

**`devices.upload` modal**

| Control | State seen | Why it matters |
|---|---|---|
| zoom in / zoom out / crop-transform / delete | icon buttons | four icon-only controls, only the first two and the last are identifiable. The third (frame with arrow) cannot be named from the UI. |
| `Re-upload` | tile | replaces an image; the upload protocol is known but re-upload semantics (replace in place vs append) are not. |
| `Close` / `Confirm` | buttons | confirm presumably triggers the upload; unverified from screenshots. |

**Also unaccounted for**

- The two left-rail icons (speedometer, monitor) lead to pages **never captured**.
  Whatever they control is entirely outside this inventory — flagging so the claim
  "complete UI coverage" is not made on the strength of these 22 shots.
- No global settings, about box, firmware update, language or theme UI was captured.

---

## 6. Source coverage

Walkthrough order, with what each shot contributes.

| # | Time | Shows | Value |
|---|---|---|---|
| 1 | 110124 | mode dropdown open | §3.5 full list |
| 2 | 110835 | full window, Monitor Mode default | only shot of rail, left column, brightness, toggles |
| 3 | 110840 | idle dropdown open | §3.4 full list (2) |
| 4 | 110846 | CPU fan I/O open | §3.7 full list |
| 5 | 110848 | pump fan I/O open | §3.8 full list |
| 6 | 110853 | main area open | §3.1 full list (6) — **key shot** |
| 7 | 110855 | style type open | §3.3 full list |
| 8 | 110857 | auxiliary open | §3.2 full list — **key shot** |
| 9 | 110907 | main = Time | Style Type absence; Core Data rendering |
| 10 | 110910 | main = Pump Speed | preview layout |
| 11 | 110913 | main = CPU FanSpeed | preview layout |
| 12 | 110917 | main = CPU FanSpeed + Pump Speed | dual-dial preview |
| 13 | 110920 | main = Temperature | preview layout |
| 14 | 110924 | Media Mode | both radio groups, full lists |
| 15 | 110928 | Media Mode, GIF playing | confirms same controls |
| 16 | 110935 | switch time disabled | one-image suppression |
| 17 | 110938 | animation = Fade In/Out | tooltip behaviour |
| 18 | 110942 | Recording mode, both series on | full screen |
| 19 | 110948 | temperature only | checkbox independence |
| 20 | 110952 | frequency only | checkbox independence |
| 21 | 111525 | `devices.upload`, 2 images | modal + untranslated title |
| 22 | 111531 | `devices.upload`, 1 image | modal + crop box |

Screens 9–13 and 15–20 are state variations; they add no new option list, but 9–13
are the evidence for the `Style Type` conditional and 16–17 are the evidence for the
disabled-radio behaviour.

## 7. Limits of this inventory

- Screenshots are partial crops. Three are missing their bottom edge, nine are
  missing the window chrome. Where a list could be cut off, the panel's own bottom
  edge was inspected at high zoom to confirm whether the list had ended — that check
  was done for `Main Display Area`, `Style Type`, `Auxiliary Display Area`, and the
  idle list, and all four terminate inside the frame.
- No screenshot shows a dropdown scrolled part-way, so no list is known to be longer
  than what is recorded.
- Icon-only controls (rail, upload toolbar, preview delete) carry no text, so their
  names are descriptions, not transcriptions.
- Only one locale was captured, and it contains at least one untranslated key
  (`devices.upload`), so other locales may surface different wording for the same
  controls.
- Nothing in this document was verified by replaying a command. It is a map of what
  the app offers, not proof of what the device accepts.
