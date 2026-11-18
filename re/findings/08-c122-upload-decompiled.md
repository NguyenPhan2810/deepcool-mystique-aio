# C122 upload state machine — decompiled from vendor code

Source: `C:\DeepCool\resources\app.asar.unpacked\resources\C122\index.node`
Function: `FUN_180002420` at `0x180002420` (file offset `0x230e` maps into it)
Tool: REA 6.7.0 → Ghidra 12.1.4, Windows x64
Log: `re/work/rea/C122-dcl-frame.md`

This is the **first vendor-side confirmation** of the image upload flow: not a third-party
capture, not documentation, but DeepCool's own shipped code. It is for the `0x22` family, not
the MYSTIQUE, so treat the *shape* as authoritative and the *sizes* as family-specific.

## What the function does

A three-phase file upload, exactly matching the error strings the native survey found
(`send start command failed`, `send file command failed`, `send finish command failed`).

### Phase 1 — start

One fixed **512-byte** packet:

```
off   size  content
0     6     "Start" 0x01
6     4     total image length, u32 LE
10    2     checksum: sum of all image bytes (u16 wrap)
12    2     packet count = ceil(total_length / chunk_size)
14    2     unused / zero
16    496   zero padding
```

The chunk size is a function parameter (`param_5`), and the packet count is computed with a
`ceil(total / chunk)` in floating point — visible as the `ceil(...)` and the clamp to
`0xffff`.

### Phase 2 — data packets, one per chunk

```
off   size  content
0     5     "trans"
5     2     packet index, 1-based
7     N     chunk payload
```

Sent repeatedly, incrementing the index. The code allocates a buffer per packet, copies the
chunk, and loop-retries on the transport status codes `-7` / `-10` (short write / timeout)
until either a positive transferred count or a hard error.

### Phase 3 — finish

One fixed **512-byte** packet:

```
off   size  content
0     10    "DCLdfinish"
10    502   zero padding
```

## Why this matters for the MYSTIQUE work

1. **`DCLd` is a shared prefix, and the terminator is `DCLdfinish`.** The public MYSTIQUE
   protocol describes a 64-byte header starting `DCLd`. This code builds the *terminator*
   `DCLdfinish` the same way. The two are consistent: `DCLd` opens, `DCLd`+`finish` closes.
   That is now corroborated from vendor code rather than only from captures.

2. **Families differ in chunking, not in vocabulary.** `C122` uses 512-byte control packets
   and an explicit `trans`+index chunk loop. The MYSTIQUE is described as sending a 64-byte
   header glued to a whole JPEG in one transfer, with no `trans` packets. Same markers,
   different transport. Do not assume the `C122` sizes transfer to `3633:0009`.

3. **The checksum is a plain 16-bit sum**, consistent with the 48-byte packet checksum and
   with the `DCLd` header's JPEG checksum. No CRC anywhere in the image path.

4. **Errors are strings, not codes.** The function returns `"success"` or one of
   `"send start command failed"`, `"send file command failed"`, `"send finish command
   failed"`. A caller that only checks for a thrown exception would miss failures entirely.

## Confidence

| Claim | Confidence | Basis |
|---|---|---|
| Three-phase Start → trans → finish | High | decompiled control flow, both branches |
| 512-byte control packets | High | `0x1f0` literal, repeated buffer growth to 512 |
| Header field order and widths | High | explicit stores at increasing offsets |
| Chunk size value | Unknown | passed in as `param_5`; not a constant in this function |
| `DCLdfinish` terminator | High | built from `movabs` immediates |
| Applies to MYSTIQUE | **Unknown** | this is family `0x22`, not `0x0009` |

## Reproduction

```powershell
$env:GHIDRA_INSTALL_DIR='C:\tools\ghidra_12.1.4_PUBLIC'
$env:JAVA_HOME='C:\Program Files\Eclipse Adoptium\jdk-25.0.5.8-hotspot'
rea decompile "C:\DeepCool\resources\app.asar.unpacked\resources\C122\index.node" `
  --file-offset 0x230e --format md
```

Note: REA 6.7 rejects Ghidra 12.2 and JDK 21 — it wants Ghidra **12.1.x** and JDK **25+**.
Both are now installed at `C:\tools\ghidra_12.1.4_PUBLIC` and
`C:\Program Files\Eclipse Adoptium\jdk-25.0.5.8-hotspot`.
