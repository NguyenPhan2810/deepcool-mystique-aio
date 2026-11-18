# 05 — The bytecode layer: mechanism, versions, and why static RE stops here

Companion to [`01-electron-protocol.md`](./01-electron-protocol.md), which covers the
plaintext renderer/preload surface. This document is deliberately short: it records the
mechanism, the exact runtime versions, what could be recovered *without* disassembly,
what could not, and the command a follow-up agent should run.

Short version: the vendor's main process is **V8 Ignition bytecode in a custom
`vm.Script` cachedData container**, there is **no disassembler in the shipped V8 build**,
and the popular public tooling targets an older bytecode format. This is a hard stop for
static source recovery, and it is the reason the protocol has to be derived from wire
traffic.

---

## 1. Compilation mechanism

Not `bytenode`. Not `v8-compile-cache`. A hand-rolled loader:

`out/main/index.js` (71 bytes, the whole file):
```js
"use strict";
require("./bytecode-loader.js");
require("./index.jsc");
```

`out/main/index.js` is `package.json`'s `main` (`package.json:5`:
`"main": "./out/main/index.js"`), so this runs before anything else.

`out/main/bytecode-loader.js:32-51` registers a module extension:
```js
Module._extensions[".jsc"] = function (module, filename) {
  const bytecodeBuffer = fs.readFileSync(filename);
  setFlagHashHeader(bytecodeBuffer);          // copies the flag hash from a dummy script
  const length = buffer2Number(getSourceHashHeader(bytecodeBuffer));
  let dummyCode = "";
  if (length > 1) dummyCode = "\"" + "\u200b".repeat(length - 2) + "\"";
  const script = new vm.Script(dummyCode, { filename, cachedData: bytecodeBuffer });
  if (script.cachedDataRejected) throw new Error("Invalid or incompatible cached data");
```
Details that matter:

* It forces two V8 flags first: `--no-lazy` and `--no-flush-bytecode`
  (`bytecode-loader.js:7-8`) — i.e. the file was produced by a build that eagerly
  serialised **every** inner function, so the bytecode is dense and complete.
* The source hash is *not* validated against real source: the loader fabricates a dummy
  source string of the right length out of zero-width spaces (U+200B). Only the 4-byte
  **flag hash** at offset 12 is patched, copied from a throw-away `vm.Script("")`.
  Consequence: the `.jsc` is *portable across identical V8 builds* but not across V8
  versions.
* `cachedDataRejected === true` is a hard failure, so running under the wrong V8 gives a
  clean, immediate error rather than garbage.

### Exact file inventory

| Path | Size | Role |
|---|---|---|
| `out/main/index.js` | 71 B | entry, requires loader + bytecode |
| `out/main/bytecode-loader.js` | 2 278 B | the `.jsc` loader (§ above) |
| `out/main/index.jsc` | 2 830 472 B | **the entire main process**, bytecode |
| `out/preload/index.js` | 791 B | plaintext preload (3 IPC methods) |
| `out/preload/worker.js` | 60 188 B | plaintext; update-progress page, not the panel |

There is **exactly one** `.jsc` in the archive. Verified by scanning the full entry list
(`re/work/asar-list.txt`, 30 449 entries) for `.jsc`, `bytenode`, `bytecode`,
`v8-compile-cache`: the only matches are `out/main/index.jsc` and
`out/main/bytecode-loader.js`. `node_modules` contains no `bytenode` and no
`v8-compile-cache` (directory search over the extraction returned nothing).

The renderer is **not** bytecode: `out/renderer/assets/*.js` are readable, non-minified
ESM bundles with Chinese source comments intact, which is why doc 01 could be written
from them. So the "bytenode" impression comes from this single main-process file.

---

## 2. Runtime versions (must match exactly to decode)

Established by *running the shipped binary*, not by inference:

```
PS> $env:ELECTRON_RUN_AS_NODE=1; & C:\DeepCool\DeepCool.exe -e "console.log(process.versions)"
{
  node: '18.12.1',
  v8: '11.0.226.20-electron.0',
  electron: '23.3.13',
  modules: '113',
  napi: '8',
  ...
}
```

`ELECTRON_RUN_AS_NODE=1` makes `DeepCool.exe` behave as plain Node — no window, no app
code, safe to call.

* **Electron 23.3.13**
* **V8 11.0.226.20-electron.0**
* Node 18.12.1, N-API 8

Cross-checks: `package.json:10` pins `"node": ">=18.0.0 <19"`; the loader's own
`v8.setFlagsFromString` calls only work on 18.x/Electron 23-era V8.

**This is the single most important number in this document.** Any decoder must
implement the V8 11.0 Ignition opcode table, or run inside this exact binary.

---

## 3. What *did* work: executing the bytecode under its own runtime

The `.jsc` loads and runs when executed inside `C:\DeepCool\DeepCool.exe` in Node mode.
I instrumented `Module._load` and replaced `electron` plus every vendor addon with
recording stubs (`re/work/probe-main2.cjs`, log at `re/work/probe-log2.txt`).

Observed, verbatim from `re/work/probe-log2.txt:9-19`:
```
REQUIRE.NATIVE ../../resources/C122/index.node        <- out\main\index.jsc
REQUIRE.NATIVE ../../resources/L122/index.node        <- out\main\index.jsc
REQUIRE.NATIVE ../../resources/L086/index.node        <- out\main\index.jsc
REQUIRE.NATIVE ../../resources/L136/L136.node         <- out\main\index.jsc
REQUIRE.NATIVE ../../resources/CH690/CH690.node       <- out\main\index.jsc
REQUIRE.NATIVE ../../resources/L142/index.node        <- out\main\index.jsc
REQUIRE.NATIVE ../../resources/opencv/pixel.node
REQUIRE.NATIVE ../../resources/capture/wincapture-node.node
REQUIRE.NATIVE ../../resources/event/ready.node
REQUIRE.NATIVE ../../resources/ffmpeg/ffmplayer.node
REQUIRE.NATIVE ../../resources/system/system_info_x64.node
```

Two useful facts fall out:

1. **All six LCD addons load unconditionally at startup**, from the single main bundle,
   before any device enumeration. They are a fixed library, not a per-product selection.
   (Used in doc 01 §8.)
2. The bundle reaches its device/driver registration phase — it prints
   `[Device] register device events` and `[Driver] register driver events` through
   electron-log — then throws `TypeError: undefined is not a function` at bytecode
   offset `1:2765196` (≈97 % through the file). The failing call is in a
   stub-dependent path, not in the bundle itself; further stubbing would be needed to
   step past it.

That last point is the practical ceiling of the black-box route: with enough stub
fidelity the process would run, and `ipcMain.handle` plus addon calls could be recorded
live. It is a viable second route if someone wants behavioural data without a decoder
(see §6).

---

## 4. Why decoding failed (three separate blockers)

**(a) The shipped V8 has no disassembler.**
```
PS> & C:\DeepCool\DeepCool.exe --allow-natives-syntax -e "function f(a){return a+1};
     console.log(%DisassembleFunction(f)); console.log(%GetOptimizationStatus(f))"
start
undefined
262145
```
`%DisassembleFunction` returns `undefined` and prints nothing; `%GetOptimizationStatus`
works. So the runtime exposes V8's runtime functions but was linked with
`v8_enable_disassembler = false` (standard for release Electron). There is no
`--print-bytecode` path either: bytecode is **deserialised**, never generated, so the
printing hook never fires.

**(b) `view8` is not installable as described.**
```
PS> python -m venv re\work\venv
PS> re\work\venv\Scripts\python.exe -m pip install view8
ERROR: Could not find a version that satisfies the requirement view8 (from versions: none)
ERROR: No matching distribution found for view8
```
It is not on PyPI; it would have to be cloned from its GitHub repo. More importantly,
view8 pins a **V8 8.x** opcode table (Node 16 / Electron 13–15 era). V8's Ignition
opcode numbering and operand encoding change between majors; V8 **11.0** is far outside
that range. Expect it to desynchronise within the first few functions.

**(c) The container is custom, not a `.jsc` produced by `bytenode`.**
The flag-hash patch at offset 12 and the fabricated source hash are specific to this
loader. Tools that expect a bytenode header (`0xC0DE0000`-style magic, or a plain
`v8::ScriptCompiler::CachedData` from `bytenode compile`) will mis-parse this file.

Net: **static source recovery is blocked with the tooling available here.** Not
"unlucky" — blocked by (a) and (b) simultaneously.

---

## 5. What the bytecode still gave us without disassembly

String literals survive in the serialised constant pool, so a byte-level strings pass is
productive even when code is not. Method used: `re/work/carve-strings.mjs` (printable-run
carver, min length 5) over `out/main/index.jsc` → 10 231 string runs
(`re/work/index-strings.txt`), then filtered to identifier-shaped tokens.

This recovered the **class and method inventory of the whole main process** — enough to
name the architecture even though bodies are opaque:

* **Controllers** (per family): `MystiqueController`, `MystiquePlusController`,
  `C122Controller`, `L142Controller`, `Ch690Controller`, `LsseController`, `AkController`,
  `AgController`, `LpController`, `Ch360SeriesController`, `AppController`.
* **USB services**: `MystiqueUsbService`, `MystiquePlusUsbService`, `L086UsbService`,
  `L122DBService`, `LsseUsbService`, `AgUsbService`, `Ak500sUsbService`,
  `Ak620UsbService`, `Ak620ProUsbService`, `Ch760RsbService`, `Ch690RsbService`.
* **Generic device plumbing**: `BaseDevice`, `checkSendResponse`, `deviceDB`,
  `SupportedDevice`, `TRANSFER_TYPE`, `transportFile`, `transportIpc`, `protocol`,
  `normalizeForUpload`, `uploadChunk`, `batchUpload`, `uploadBin`, `uploadGif`,
  `getImageFrameFromPlayerId`, `removeImageToPlayer`, `convertGifToJpeg`,
  `mergeJpegToGif`, `getGifImageData`, `deviceRotateC`, `numToArr`, `rotateScreen`.
* **Per-family display config validators/defaults**: `c122DeviceConfigValid`,
  `l122DeviceConfigValid`, `l086DeviceConfigValid`, `l136DisplayConfigValid`,
  `l142DisplayConfigValid`, `ch690DisplayConfigValid`, `l142DisplayConfigDefault0/1`,
  `ch690DisplayConfigDefault0`, `l136DisplayConfigDefault0`, `l122ModelConfigDefault`,
  `deviceDefaultConfig`, `currentDisplayConfig`, `gifRgbStatus`, `screenUndervoltage`,
  `screenIndex`, `screenDelay`, `screenBrightness`, `noLoadScreen`, `motionModeScreen`.
* **IPC channel literals** — the `mystique/*`, `app/*`, `sys/*`, `worker/*`,
  `c122/*`, `ch690/*` families listed in doc 01 §3.

Two extra names are worth flagging because they do not appear anywhere in the plaintext
renderer: **`BaseDevice`** and **`checkSendResponse`**. Together with the `webusb`
literal, they are the strongest hint that this build contains a *generic, JS-level USB
device class with its own request/response/ack logic* — which is where the MYSTIQUE
protocol most likely lives.

**Caveat, stated plainly:** names are not behaviour. `uploadChunk` proves chunking
exists somewhere in JS or is called into; it does not prove the chunk size, the command
byte, or the pacing.

---

## 6. Answer to the open question: does this build support the MYSTIQUE?

**Yes — as a first-class product, and not through any vendor addon.** Evidence:

| # | Evidence | Where |
|---|---|---|
| 1 | Version is `1.2.12`; the archive carries `resources/fw/MYSTIQUE_2.70.bin` | `package.json:3`; `re/work/asar-list.txt` |
| 2 | `"MYSTIQUE"` is in the supported-product list, with its own `router.push('/devices/mystique/…')` branch | `index-94d21bab.js:169`, `:468-470` |
| 3 | Dedicated route `mystique/:serialNumber` | `index-c58f7fdf.js:42488` |
| 4 | Dedicated store with the full `DeviceInfo` shape | `index-c58f7fdf.js:36356-36456` |
| 5 | Dedicated IPC namespace `mystique/*` (8 channels) | `index-c58f7fdf.js:35515-35544` |
| 6 | Main-process classes `MystiqueController`, `MystiqueUsbService`, `MystiqueAppService` (+ `MystiquePlus*`) | `re/work/index-strings.txt` |
| 7 | UI is model-aware: `aspectRatio: {mystique: 4/3 or 3/4, "mystique-plus": 1}` | `index-366c4649.js:52395-52398` |

**And the transport is not an addon.** No shipped addon matches PID `0x0009` (native
survey: C122=`0x22`, L122=`0x26`, L086=`0x27`, L136=`0x2E`, CH690=`0x30`,
L142=`0x31`/`0x32`), and my own byte scans for the `HIDC` marker and for a 48-byte packet
builder found nothing conclusive. Meanwhile the main bundle names `webusb`,
`BaseDevice`, `checkSendResponse`, depends on `usb` 2.14.0, and emits a `usb-change`
event that the renderer consumes. `usb` ships `dist/webusb/webusb-device.js` — a
libusb-backed WebUSB facade.

> **Inference (strong):** MYSTIQUE is driven from main-process JS over libusb via the
> `usb` package's WebUSB-shaped API, in a `BaseDevice` subclass. The addons belong to
> other families. This is consistent with the device presenting as vendor-specific
> bulk endpoints, with no HID report protocol, and with the C# WinUSB client working.

### One honest contradiction to hand forward

`AA 2E` *does* occur as a deliberate header write in two addons:

```
C122/index.node @ 0x12ef : 66 c7 00 aa 2e   ; mov word ptr [rax], 0x2eaa   (after a 2-byte alloc)
L086/index.node @ 0x2734 : 66 c7 00 aa 2e   ; same, after `mov ecx,2 / mov edx,1` + call
```
Found with `re/work/find-bytes.mjs`. The native survey reported that the `AA 2E`
framing appears in no binary. One of the two is wrong, or the magic is shared with a
different family's frame. It does not change the PID conclusion, but it should be
resolved by whoever next has a disassembler open — if C122 or L086 really does build
`AA 2E …` frames, one of them may be a sibling protocol worth diffing against the
MYSTIQUE.

---

## 7. What a follow-up agent should run to go further

In preference order. Expect (1) to be the only realistic route.

1. **Behavioural, no decoder needed** — finish the instrumented run.
   `re/work/probe-main4.cjs` already loads the real `index.jsc` under the real V8 with
   `electron` and all addons stubbed, and gets to `[Device] register driver events`
   before a stub-dependent `TypeError` at `1:2765196`. Next steps: keep resolving that
   call by widening the stub (log which `makeAny` proxy is invoked immediately before
   the throw), then read `ipcHandlers` and the addon call log. Once
   `ipcMain.handle("mystique/upload-image", fn)` is reachable, `fn` can be called
   directly with a synthetic payload — the request/response flow then becomes observable
   without any disassembly.
   ```
   $env:ELECTRON_RUN_AS_NODE=1; & C:\DeepCool\DeepCool.exe re\work\probe-main4.cjs
   ```

2. **A real decoder for V8 11.0.** Requires an opcode table for
   `11.0.226.20-electron.0` (generate it from the matching V8 tag; Electron 23.3.13 uses
   V8 11.0.226.20) plus a parser for `v8::ScriptCompiler::CachedData`: header, flag hash
   (patch site offset 12), source hash, then the serialised
   `SharedFunctionInfo`/`BytecodeArray`/`FixedArray` constant-pool graph. Public
   `view8`-class tools will **not** work unmodified; they would need that table swapped
   in. Build the table with the V8 `bytecodes.h` from `11.0.226.20`
   (`v8/src/interpreter/bytecodes.h`), where operand types and sizes are declared.

3. **`d8` from a debug build of the same V8** — a debug `d8` links the disassembler, so
   `--print-bytecode --print-bytecode-filter=*` on the deserialised script prints real
   Ignition assembly. Requires building V8 11.0.226.20 from source (`gn gen` with
   `is_debug=true v8_enable_disassembler=true`), which is hours of build time, and
   `cachedData` still must be produced/consumed by that exact build.

4. **Cheapest sanity check, if you only want the device filter table** — the table is a
   JS literal; the *string* form of a VID/PID table would have survived in the constant
   pool as numbers, which it does not (integers are Smi operands, see §5 caveat). Do not
   expect a shortcut here. The wire-traffic route is faster.

---

## Appendix — artefacts produced

| Path | What |
|---|---|
| `re/work/asar/` | full extraction (git-ignored) |
| `re/work/asar-list.txt` | 30 449 archive entries with packed/unpacked flag |
| `re/work/index-strings.txt` | 10 231 carved strings from `index.jsc` |
| `re/work/carve-strings.mjs`, `filter-ids.mjs`, `ctx.mjs`, `slice.mjs`, `find-bytes.mjs`, `bin-strings.mjs` | small, reproducible analysis scripts |
| `re/work/probe-main{,2,3,4}.cjs` | instrumented-run harness (final: `probe-main4.cjs`) |
| `re/work/probe-log{,2,3,4}.txt` | its output |
| `re/work/addon-strings/*.txt` | strings from the six LCD addons |
| `re/work/rea/analyze.json` | `rea analyze-javascript-application` output — **not usable as-is**: the run streamed progressive JSON and the file reached 517 MB (6389 lines) without reaching a digestible result. Re-run with the app-only subtree (`re/work/asar/out`) rather than the whole 33 k-file extraction. |
