# `0x04` is four bytes — style at index 1, aux at index 3

Date: 2026-11-18. Captures: `re/captures/usbpcap-aux.pcap` and `re/captures/usbpcap-style.pcap`,
both recorded while the owner drove the vendor app. Decoded with `re/tools/diff_blocks.py`.

## The layout

```
index  0      1       2   3
      main   style    ?   aux
```

Style: `0` Normal, `1` Ring, `2` Stopwatch — and it only has an effect when main is
`CPU Frequency`.

### Style capture

| Time | Payload before → after | Action |
|---|---|---|
| 29.40 s | `00 00 00 02` → `00 01 00 02` | Normal → Ring |
| 37.28 s | `00 01 00 02` → `00 02 00 02` | Ring → Stopwatch |

### Aux capture

| Time | Payload before → after | Action |
|---|---|---|
| 25.64 s | `05 00 00 02` → `00 00 00 02` | Main: Temperature (5) → CPU Frequency (0) |
| 35.78 s | `00 00 00 02` → `00 00 00 00` | Aux: Core Data (2) → Voltage Mode (0) |
| 47.03 s | `00 00 00 00` → `00 00 00 01` | Aux: → System Monitor (1) |
| 57.21 s | `00 00 00 01` → `00 00 00 02` | Aux: → Core Data (2) |

Index 2 has never been observed non-zero.

## Style Type verified from our code

The app was pointed at the panel and Style Type was driven through our own encoder:
**Ring** and **Stopwatch** both rendered. Combined with the captured byte transitions, `0x04`
is now fully mapped apart from index 2.

State of the four bytes after this pass:

| Index | Field | Status |
|---|---|---|
| 0 | Main Display Area | verified on hardware |
| 1 | Style Type | verified on hardware |
| 2 | — | never non-zero; **Unknown** |
| 3 | Auxiliary Display Area | verified on hardware |

Both style fields render only when main is `CPU Frequency`, matching the vendor UI.

## A misattribution worth recording

In the earlier settings capture, `0x04` byte 1 moved and was attributed to the auxiliary
selector. It was **Style Type**. That single wrong inference cost several test cycles, because
the layout was assumed to be two bytes (`[main, aux]`) rather than four.

The lesson: a diff tells you *that* a byte changed, never *which control* changed it. Only the
labelled capture does.

## Corroboration from the original repo

`Mystique.Core.Device/MystiquePackets.cs:121` sends:

```csharp
new(0x04, [0x05, 0x00, 0x00, 0x01], 10),
```

which is **main = Temperature (5), style = Normal (0), aux = System Monitor (1)**. The C#
implementation had the layout right; the 240-derived rewrite collapsed it to two bytes.

## Consequences

- `DisplayAreas` must produce `[main, style, 0, aux]`.
- The init sequence's `0x04` must carry the current main/style/aux, not an empty payload.
- Aux is a **style selector for the secondary area**: `0` Voltage Mode, `1` System Monitor,
  `2` Core Data. Only renders when main is `CPU Frequency`.
- Style Type is a **style for the primary area**: `0` Normal, `1` Ring, `2` Stopwatch. Only
  visible when main is `CPU Frequency`.
- `0x03` remains `Unknown`.

