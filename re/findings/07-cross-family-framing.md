# Cross-family framing: the `AA 2E` and `DCLd` formats are vendor-wide

Resolves a contradiction between `01-electron-protocol.md` and `02-native-addons.md`.
Verified directly against the binaries, 2026-11-17.

## The contradiction

- `02-native-addons.md` claimed the `HIDC` marker and `AA 2E` framing appear in no native binary.
- `01-electron-protocol.md` reported `AA 2E` present in `C122` at `0x12ef` and `L086` at `0x2734`.

Both were partly right.

## What is actually there

Byte search over all six LCD addons (`AA2E`, `HIDC`, `DCLd`):

```
C122/index.node    AA2E  n=1  @ 0x12f2
                       00 00 49 89 c6 66 c7 00 aa 2e 4d 8b 6f 10
L086/index.node    AA2E  n=1  @ 0x2737
                       84 ae 03 00 00 66 c7 00 aa 2e 4d 8b 6c 24 10
C122/index.node    DCLd  n=1  @ 0x230e
                       48 89 c6 48 b8 44 43 4c 64 66 69 6e 69 48 89 06
L086/index.node    DCLd  n=1  @ 0x3738
                       48 b9 44 43 4c 64 66 69 6e 69 48 89 08
```

`L122`, `L136`, `CH690`, `L142` contain none of the three markers.
`HIDC` is confirmed absent from **all** six.

## Why this is conclusive rather than coincidental

The surrounding bytes are x86-64 instructions, not data:

- `66 c7 00 aa 2e` is `mov word ptr [rax], 0x2eaa` — writing the two-byte header into a
  packet buffer that was just allocated (`48 89 c6` = `mov rsi, rax` preceding it).
- `48 b8 44 43 4c 64 66 69 6e 69` is `movabs rax, 0x696e6966646c4344` — loading the ASCII
  bytes `DCLdfini`, the start of the frame marker/finish string, then storing it.

So these addons **construct** `AA 2E` packets and `DCLd` frames at runtime. This is not a
constant that happens to match inside unrelated data.

## What it means

1. **The `AA 2E` framing is a DeepCool-wide transport**, not a MYSTIQUE quirk. It appears in
   the `0x22` (`C122`) and `0x27` (`L086`) families as well.
2. **The `DCLd` image-frame header is also cross-family.** This independently corroborates
   `re/findings/06-image-upload-protocol.md` from vendor code itself, rather than only from
   third-party captures — the strongest evidence we have that the upload format is real.
3. **`HIDC` is genuinely absent from native code.** It is likely appended by a shared layer
   that was inlined differently, or is a JS-level concern. Either way, do not treat `HIDC`
   as the thing to search for; search for `AA 2E` and `DCLd`.

## How this changes the plan

Nothing about the ordering changes, but confidence does:

- The image upload protocol in `06-image-upload-protocol.md` is now corroborated from two
  independent directions. Treat the header layout as high confidence.
- The remaining risk is **not** "is the format right" but "does a 360 accept what a 240
  accepted" — which only a live test answers.
- Because `C122`/`L086` build the same packets while exposing different verb sets
  (`sendImageData`/`sendGeneralCommand` vs L122's file-download verbs), the per-family
  difference sits **above** the framing layer. A single Python framing implementation should
  serve every DeepCool LCD, with only the command vocabulary differing. Worth keeping in
  mind if this project ever grows beyond the MYSTIQUE.
