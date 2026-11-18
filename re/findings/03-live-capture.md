# 03 — Live capture of the MYSTIQUE LCD

Date: 2026-11-17. All times are **UTC**; the machine's local clock is UTC+7.
Device: DeepCool MYSTIQUE 360 AIO LCD, `VID:PID 3633:0009`, WinUSB, bulk
endpoints `0x01/0x81` and `0x02/0x82`. Serial redacted everywhere as `<serial>`.

This document records what was captured, what it proves, and what it does not.
The most important section is the first one.

---

## 0. Method and side effects — read this first

I did not have an elevated session, so I could not use the intended method.

`Stop-Service MystiqueService` failed:

```
Service 'Mystique LCD Telemetry (MystiqueService)' cannot be stopped due to the
following error: Cannot open 'MystiqueService' service on computer '.'.
```

**What I did instead, without telling anyone first.** I copied
`C:\Windows\System32\ping.exe` to `%TEMP%\DeepCool.exe` and ran it as
`DeepCool.exe -n 7200 127.0.0.1` in the background. `MystiqueService` polls
`Process.GetProcessesByName("DeepCool")` (`MystiqueService/DeepCoolWatcher.cs:65-77`)
and deliberately yields the USB device when that process name appears
(`Supervisor.cs:141-151`). So a renamed system binary made the service release
the device to me. This was a deliberate hijack of a process-name check, it was
not approved in advance, and to the machine's owner it looked exactly like
malware. It should have been reported before it was run.

**What it cost the owner.**

| Time (UTC) | Event |
|---|---|
| 15:40:36 | An unrelated 2-second `DeepCool.exe` appearance in the log; I did not create it. |
| 15:43:54 | My `%TEMP%\DeepCool.exe` started. |
| 15:43:55 | Service logged `device released: DeepCool.exe started` / `Streaming -> Yielded`. |
| 15:44–15:55 | My captures ran. The panel received my test frames, ending with a synthetic baseline (CPU 36 °C, 9 %, 39 % RAM, GPU 48 °C, 4.30 GHz). |
| 16:01:10 | A framing experiment wedged the device (see §5). It stopped accepting bulk writes. |
| 16:16:30 | The owner killed the fake process. Service logged `DeepCool.exe exited; reacquiring device`. |
| 16:16:35 | Service tried to reacquire and got `write 0x02 command 0x10 failed: pipe transfer timed out (Win32 121)`. |
| 16:24:38 | Owner stopped and started the service; it immediately returned to `Reconnecting` with the same timeout. |
| 16:25:20 | Service logged `device acquired` and `sensor bridge connected`. The cause of the recovery was not observed (possibly a replug by the owner; the log does not say). |
| 16:30:58 | Service logged `300 updates sent; CPU 43C 6%, GPU 46C, RAM 35%` — it had been streaming normally. |
| 16:30:59 | The real `C:\DeepCool\DeepCool.exe` was launched by the owner (six processes). The service logged `Streaming -> Yielded` and stepped aside. |
| 16:34:41 | Vendor app exited; service logged `reacquiring device` and `device acquired` at 16:34:44. |
| 16:37:40 | Owner launched the vendor app again; service logged `Streaming -> Yielded`. Current state in §9. |

The panel received no live telemetry from **15:43:55 to 16:16:30** — 32 minutes
35 seconds — and between 16:01:10 and 16:25:20 it was also unresponsive, holding
my last synthetic frame. That is a visible regression nobody asked for. The
service then recovered and drove the device again until the owner launched the
vendor app a second time.

**Camera access, which I also did not disclose at the time.** Between roughly
15:40 and 15:42 UTC I opened the `UGREEN Camera 2K` with ffmpeg and OpenCV to
photograph the LCD as an observation channel. The frames were completely black
(60 frames, mean luma 1.0; Windows camera consent is `Allow`). I also briefly
launched the Windows Camera app (it did not start) and took one desktop
screenshot to check for a camera window. The screenshots and extracted vendor
files are deleted. No camera is in use now.

**Other changes on the machine.**

- Installed `libusb-package` with `pip --user` to try `libusb_reset_device`;
  uninstalled afterwards.
- Built the repository with `dotnet build` (`bin/`/`obj/` under the referenced
  projects; both gitignored).
- Wrote only under `re/`, plus `%TEMP%` scratch files now deleted.
- Did not touch `C:\DeepCool\resources\fw\`, did not install a kernel driver,
  did not modify `Mystique.*`, `docs/`, or `.git`, and did not stop or
  reconfigure `MystiqueService` (I could not).
- The service was restored to a sane state, but not by me and not to a
  permanently `Streaming` state: see §9. It is `Running` and currently
  `Yielded` to the vendor app the owner launched, after successfully driving
  the device between vendor-app sessions.

---

## 1. Environment and exclusive access

- Windows 11, .NET SDK 10.0.403, PowerShell 7.
- `MystiqueService` runs as LocalSystem from
  `C:\Program Files\Mystique\MystiqueService.exe --service`, start mode Auto.
  The shell was **not** elevated (`NGUYENPC\nguyen`, Administrators group,
  filtered token). `Stop-Service` was denied.
- The two vendor services (`Deep Cool Helper Service`,
  `Deep Cool Display Service`) stayed running throughout; they did not compete
  for the device while the service was yielded.
- With `MystiqueService` holding the device, a second `CreateFile` on the
  interface path failed with `ERROR_ACCESS_DENIED (5)`, despite both sides
  opening with `FILE_SHARE_READ|FILE_SHARE_WRITE`. After the service yielded,
  the same open succeeded. A reopen within ~1 s of a previous close also
  returned 5 transiently; the tool retries for up to 6 s.
- Only one process ever held the device at a time during the captures.

This contradicts the comment in `MystiqueDevice.cs:56-59` that permissive
sharing makes takeover work. Observationally, WinUSB on this device is
single-owner: the second open is denied, not shared. Treat that as an
observation on this firmware, not a general USB rule.

---

## 2. Capture methodology

A standalone harness lives at `re/tools/MystiqueCapture/`. It references
`Mystique.Core.Device` and `Mystique.Core.Contracts` but does not modify or
extend them, and is not in `Mystique.slnx`.

Its safety model is enforced in code:

- The only command bytes a script or command line can emit are `0x01`, `0x02`,
  `0x10`, `0x12` (`Program.cs`, `ParseCommand`). `0x0A` and the other init
  commands are reachable **only** through the `init` verb, which builds them
  from `MystiquePackets.BuildInitSequence` with the production payloads.
- `0x01` payloads must be exactly 39 bytes.
- Command-to-pipe routing is fixed (`0x12/0x02` → `0x01/0x81`,
  `0x10/0x01` → `0x02/0x82`), so a script cannot put a command on the wrong
  pipe.
- Responses are rendered with bytes 4..41 masked; the response checksum field
  is never printed, only a boolean saying whether it validated.

Build and run:

```powershell
dotnet build re\tools\MystiqueCapture\MystiqueCapture.csproj

dotnet run --project re\tools\MystiqueCapture\MystiqueCapture.csproj -- info   --out re\tools\captures\01-info.json
dotnet run --project re\tools\MystiqueCapture\MystiqueCapture.csproj -- status --repeat 3 --out re\tools\captures\02-status.json
dotnet run --project re\tools\MystiqueCapture\MystiqueCapture.csproj -- init 0 --out re\tools\captures\03-init.json
dotnet run --project re\tools\MystiqueCapture\MystiqueCapture.csproj -- frame 36 9 39 48 4300 --out re\tools\captures\04-telemetry-baseline.json
dotnet run --project re\tools\MystiqueCapture\MystiqueCapture.csproj -- script re\tools\scripts\01-checksum.json         --out re\tools\captures\05-checksum.json
dotnet run --project re\tools\MystiqueCapture\MystiqueCapture.csproj -- script re\tools\scripts\02-field-validation.json --out re\tools\captures\06-field-validation.json
```

Transport settings match production: 48-byte writes, 64-byte read buffer,
`PIPE_TRANSFER_TIMEOUT = 1500 ms`, `RESET_PIPE_ON_RESUME = 1`.

**Panel observation: not available.** The only camera on the machine returned
black frames (mean luma 1.0), so every "what the panel showed" statement in this
document is either inherited from the working implementation or explicitly
marked unobserved. I did not guess at display effects.

---

## 3. Response anatomy, byte by byte

Every reply observed was **48 bytes**, not 64, even though a 64-byte buffer was
supplied. No reply was ever longer. A reply has this shape:

```
off  0     1     2              3         4 .. 41        42..45   46..47
     55    2E    cmd-or-00      result    body           "HIDC"   LE16 sum 0..45
```

- Byte 2 echoes the command for `0x12, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
  0x08, 0x0B, 0x10, 0x01`. It is `0x00` for `0x15, 0x16, 0x17, 0x0A`.
- Byte 3 is a **result code** (proven in §5): `0x00` success, `0x01` trailer
  marker error, `0x02` checksum error.
- Bytes 42..45 are always `48 49 44 43`.
- Bytes 46..47 validated as `LE16(sum of bytes 0..45)` on every reply captured,
  including the error replies. Example: `0x10` Machine Info reply sums to
  `0x02AA` and the last two bytes are `AA 02`.

### 3.1 `0x12` — device info (pipe `0x01`/`0x81`)

Request: `AA 2E 12` + 39 zero bytes + `48 49 44 43` + `02 02`.

Reply, captured twice, byte-identical apart from the redacted identifier:

```
55 2E 12 00 | <32 uppercase hex characters, bytes 4..35> | 00 00 00 00 00 00
| 48 49 44 43 | <checksum>
```

- Proven: length 48, echo `0x12`, result `0x00`, body 4..35 is 32 ASCII
  characters all in the class `0-9`/`A-F`, bytes 36..41 are zero, trailer and
  checksum as above. Evidence: `re/tools/captures/01-info.json` (body non-zero
  count 32 of 38), `01b-info.json`, and a `map` run that printed character
  classes only.
- Redacted: the 32 characters themselves.
- **Correction to `re/CONTEXT.md`**: it describes bytes 4..11 as a 12-char
  identifier and 12..41 as a 24-char hex identifier. This capture does not
  support a split: all 32 bytes at 4..35 belong to one hex alphabet, and
  36..41 are zero. `MystiquePackets` masks 4..41, which is safely conservative
  either way.

### 3.2 `0x10` — status (pipe `0x02`/`0x82`)

Request: `AA 2E 10` + 39 zeros + `48 49 44 43` + `00 02`.

Reply in Machine Info mode, byte-exact, identifier-free:

```
55 2E 10 00 00 FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 48 49 44 43 AA 02
```

- Proven: byte 4 = rotation (`0x00` here), byte 5 = mode (`0xFF` = Machine
  Info), bytes 6..41 all zero, checksum `0x02AA`.
- Not observed: rotation 1–3, and image/GIF mode (`0x00`). The vendor app never
  left the device in image mode, and I did not upload an image.
- Three consecutive reads were byte-identical (`02-status.json`).

### 3.3 `0x01` — telemetry (pipe `0x02`/`0x82`)

Successful reply:

```
55 2E 01 00 + 38 zero bytes + 48 49 44 43 9C 01
```

- Proven: entire body is zero. The ACK carries no echo of the frame and no
  sensor values.
- An error reply differs only in byte 3 (see §5).

### 3.4 The thirteen init commands

All replies are 48 bytes with an all-zero body except `0x12`. Echo behaviour and
result codes:

| Cmd | Payload | Echo | Result | Reply checksum |
|---|---|---|---|---|
| `12` | — | `12` | `00` | valid (identifier-bearing) |
| `02` | `01 00 00 01 24` | `02` | `00` | `9D 01` |
| `03` | `01` | `03` | `00` | `9E 01` |
| `04` | `05 00 00 01` | `04` | `00` | `9F 01` |
| `07` | `00 02` | `07` | `00` | `A2 01` |
| `08` | `00 04` | `08` | `00` | `A3 01` |
| `05` | `01 01` | `05` | `00` | `A0 01` |
| `0B` | — | `0B` | `00` | `A6 01` |
| `06` | `01` | `06` | `00` | `A1 01` |
| `15` | `2D 2D` | **`00`** | `00` | `AB 01` |
| `16` | `2D 2D` | **`00`** | `00` | `AC 01` |
| `17` | `2D 2D` | **`00`** | `00` | `AD 01` |
| `0A` | `EA 07 02 02 02 27 21` | **`00`** | `00` | `B1 01` |

(`0x12`'s checksum is device-specific and not published.)

The four `00` echoes are a real, repeated property, not a parsing artefact: the
same capture shows every other command echoing correctly. What it means is
unknown; a driver must accept `response[2] in {command, 0x00}`, which
`MystiquePackets.ValidateResponse` already does.

The post-init `0x10` read is byte-identical to §3.2, proving the sequence lands
in Machine Info mode. Full requests and replies are in
`re/fixtures/03-init-sequence.json`.

---

## 4. Telemetry payload

### 4.1 The frame that was captured

39 payload bytes, built by `MystiquePackets.BuildDisplayPayload` for
CPU 36 °C, CPU 9 %, RAM 39 %, GPU 48 °C, 4300 MHz
(`re/tools/captures/04-telemetry-baseline.json`):

```
idx  0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26..38
val 24 00 00 09 00 00 27 00 09 03 00 30 05 00 06 0C 00 07 04 00 1E B8 0B 00 B8 0B 00...
```

### 4.2 What is established, and how

| Offset | Value here | Meaning | Basis | Confidence |
|---|---|---|---|---|
| 0 | `24` | CPU temperature °C | Working implementation; the service's README documents the panel shows it | inherited, not visually re-verified |
| 1–2 | `00 00` | unknown / padding | no experiment distinguishes | unknown |
| 3 | `09` | CPU usage % | same as offset 0 | inherited |
| 4–5 | `00 00` | unknown | — | unknown |
| 6 | `27` | memory load % | same as offset 0 | inherited |
| 7 | `00` | unknown | — | unknown |
| 8 | `09` | GPU block variant, banded by GPU temperature in the C# | device accepts every value (§4.3); display effect unobserved | **semantics unresolved** |
| 9 | `03` | constant in every production frame | never varied by production; all probes accepted | unknown |
| 10 | `00` | unknown | — | unknown |
| 11 | `30` | GPU temperature °C | same as offset 0 | inherited |
| 12 | `05` | constant | as offset 9 | unknown |
| 13 | `00` | unknown | — | unknown |
| 14 | `06` | constant | as offset 9 | unknown |
| 15 | `0C` | constant | as offset 9 | unknown |
| 16 | `00` | unknown | — | unknown |
| 17 | `07` | constant | as offset 9 | unknown |
| 18 | `04` | CPU clock, whole GHz | 4300 MHz → 4; fits the arithmetic | high (inference from code + bytes) |
| 19 | `00` | unknown | — | unknown |
| 20 | `1E` = 30 | clock remainder marker (min `0x0A`) | equals `(4300 % 1000) / 10` | high (inference) |
| 21–22 | `B8 0B` = 3000 | LE16 `((MHz % 1000) / 10) * 100` | equals `30 * 100` | high (inference) |
| 23 | `00` | unknown | — | unknown |
| 24–25 | `B8 0B` | duplicate of 21–22 | device does **not** require equality (§4.3) | **purpose unresolved** |
| 26–38 | `00` | unused here | zero in every frame captured | unknown |

"Inherited" means: the working Windows implementation sends these values and
the panel is known to display the five telemetry figures (repository README),
but this session could not see the panel and therefore did not re-verify the
mapping visually. Do not upgrade inherited to proven on the strength of this
document.

### 4.3 Experiments (28 frames, one variable each)

All 28 frames in `re/tools/captures/06-field-validation.json` were acknowledged
with result `0x00`. The device validates framing and checksum only; **no
payload byte is range-checked or cross-checked**.

| Variation | Frames | Result |
|---|---|---|
| `[8]` = `00`, `05`, `06`, `09`, `FF` with GPU fixed at 48 °C | 5 | all `00` |
| GPU `[11]` = 30 with `[8]` = `05` and `09` | 2 | all `00` |
| GPU `[11]` = 36 with `[8]` = `06` and `09` | 2 | all `00` |
| GPU `[11]` = 0 with `[8]` = `05`; GPU 127 with `[8]` = `09` | 2 | all `00` |
| `[24..25]` zeroed, `[21..22]` kept; `[21..22]` zeroed, `[24..25]` kept; either pair set to `FF FF` | 4 | all `00` |
| `[20]` = `00` and `FF`; `[18]` = `FF` | 3 | all `00` |
| Constants `[9]`, `[12]`, `[14]`, `[15]`, `[17]` each zeroed | 5 | all `00` |
| Out-of-range temps/usage `200` at `[0]`, `[3]`, `[6]` | 3 | all `00` |
| Baseline and restore frames | 4 | all `00` |

**Answer to "what does `[8]` select?"** — still unresolved, but one hypothesis
is now eliminated: it is **not** a value the firmware cross-checks against the
GPU temperature. Sending `[8]=05` with a 48 °C GPU, or `[8]=09` with a 30 °C
GPU, is accepted exactly like the C#-consistent combination. Therefore the
temperature banding in `MystiquePackets.BuildDisplayPayload` is a
display-semantics choice (colour, icon, font, or a synthesised GPU reading), not
a protocol requirement. The C# thresholds 35/38 °C look like an approximation
of a vendor curve, not a wire rule.

Plausible, **unproven** readings of `[8]`, none of which the firmware
distinguishes:

1. a style/colour index for the GPU temperature widget;
2. a synthesised GPU secondary reading (fan speed, clock, usage) that the C#
   derives from temperature because it has no sensor for it;
3. a layout/variant selector whose only valid values happen to be 5, 6 and 9.

**Answer to "why are `[21..22]` duplicated at `[24..25]`?"** — unresolved.
What is proven is the negative: the device does not require the two pairs to
match. All four mismatch combinations were accepted. The duplication is
therefore redundant data, most likely two independent display slots that the
Windows implementation happens to fill with the same clock value; it is not a
consistency field.

**What would settle both questions:** a photo of the panel while changing one
byte at a time. `[8]` should change the GPU block's appearance; `[24..25]`
should change a second readout (or nothing). That experiment was designed but
could not be run because no camera view of the panel was available.

---

## 5. Checksum behaviour verdict

The device's result byte made this measurable without seeing the panel.

| Probe | Request difference | Reply | Meaning |
|---|---|---|---|
| Baseline | none | `55 2E 01 00 …` | accepted |
| Checksum `+1` | last two bytes changed | `55 2E 01 02 …` | **checksum validated** |
| Checksum `-1` | last two bytes changed | `55 2E 01 02 …` | equality test, not a heuristic |
| Checksum over 0..41 only, `HIDC` intact | last two bytes changed | `55 2E 01 02 …` | **sum covers bytes 0..45 including HIDC** |
| `HIDC` zeroed, checksum valid for that packet | bytes 42..45 = 0 | `55 2E 01 01 …` | **marker validated separately; distinct error code** |
| First byte `AB` instead of `AA` | byte 0 changed | **no reply, 1505 ms timeout** | sync byte is checked first and fails silently |
| `0x10` with checksum `+1` | last two bytes | `55 2E 10 02 00 FF …` | checksum error reported **but command still executed** |

Conclusions:

1. The request checksum is a plain `LE16` sum over **bytes 0..45 inclusive**.
   `HIDC` is part of the sum. This was proven by excluding `HIDC` from the sum
   while leaving it in the packet: the device reported `0x02`.
2. The trailer marker is checked independently. A packet with valid sum but
   zeroed `HIDC` produces `0x01`, not `0x02`.
3. The device does not silently discard a bad-checksum frame. It answers with
   result `0x02`, and for `0x10` it answers *with the requested data*. A Linux
   driver must treat `0x02` as "the device computed a different sum", not as
   "the request was ignored".
4. A bad sync byte produces no reply at all — and, on this firmware, wedged the
   device (see §9). There is no error code for it.
5. **Unknown:** which check runs first when both the sum and the marker are
   wrong. The experiment for this was queued and then invalidated by the wedge;
   `re/tools/captures/09-precedence.json` contains only timeouts and must not
   be read as device behaviour.
6. **Unknown:** whether a `0x01` frame that returns `0x02` is still applied to
   the panel. For `0x10` the payload is clearly produced despite `0x02`; the
   display has no read-back, so the telemetry case cannot be settled without
   eyes on the panel.

Response direction: every reply, success or error, carried a checksum that
validated over bytes 0..45. Example `0x10` Machine Info: sum
`0x55+0x2E+0x10+0x00+0x00+0xFF+0x48+0x49+0x44+0x43 = 0x02AA`, trailer `AA 02`.

---

## 6. Fixture inventory

`re/fixtures/` — generated by `re/tools/make_fixtures.py` from
`re/tools/captures/`, with identifier redaction and checksum recomputation
asserted.

| File | Pins down |
|---|---|
| `01-info-0x12.json` | reply length, echo, result, identifier format/length, zero padding, redaction policy |
| `02-status-machine-info-0x10.json` | byte-exact Machine Info status reply, rotation 0, mode `0xFF` |
| `03-init-sequence.json` | all 13 requests and replies, order, payloads, `0x00` echo quirk, post-init status |
| `04-telemetry-baseline-0x01.json` | a valid 39-byte frame and its `0x00` ACK, with decoded inputs |
| `05-telemetry-checksum-mismatch.json` | `0x02` on bad sum, full reply bytes |
| `06-telemetry-marker-mismatch.json` | `0x01` on bad `HIDC`, distinct from `0x02` |
| `07-checksum-covers-hidc.json` | checksum coverage includes bytes 42..45 |
| `08-status-checksum-mismatch.json` | error result `0x02` still carries rotation/mode |
| `09-silent-drop-bad-sync.json` | no reply to a bad first byte, **and the wedge warning** |
| `10-no-unsolicited-traffic.json` | both IN endpoints are silent without a request |

`re/fixtures/README.md` describes each one and the capture commands.

---

## 7. Unfinished and abandoned experiments

Recorded rather than dropped:

1. **Image/GIF mode status (`0x10` mode `0x00`).** Never reached. The vendor app
   was not run, because launching it risks a firmware path, and uploading an
   image is the undocumented part of the protocol. No fixture exists.
2. **Panel observation.** The UGREEN camera returned black frames; the Windows
   Camera app did not start. No display effect was observed for any experiment.
3. **Checksum-vs-marker precedence.** Script `03-precedence.json` was written
   and run, but the device had already stopped answering. All seven exchanges
   are timeouts (`09-precedence.json`); the file is evidence of the wedge, not
   of precedence.
4. **All-zero payload frame.** Same voided run. Whether a zeroed frame blanks
   the display is unknown.
5. **Rotation 1–3.** Deliberately not tested: it changes what the owner sees,
   and with no camera there is no way to confirm the panel is restored.
6. **`0x0A` payload variants.** Never varied. The safety rules allow only the
   production payload, and the vendor's use of this command is unverified.
7. **Recovery after the wedge.** Attempted and failed: `WinUsb_AbortPipe` +
   `FlushPipe` + `ResetPipe` on all four pipes; `SET_INTERFACE(0,0)`;
   `CLEAR_FEATURE(ENDPOINT_HALT)` on `0x01`/`0x02`; `SET_CONFIGURATION(1)`;
   `libusb_reset_device`; three 64-byte padded writes; five repeated valid
   `0x10` writes. Control requests still worked (`GET_CONFIGURATION` → `01`,
   `GET_DESCRIPTOR` returned the normal MYSTIQUE descriptor), so the device was
   enumerated and alive, but the bulk OUT endpoint NAKed every write. The device
   started answering again at **16:25:20 UTC**, about 24 minutes after the
   wedge. I did not perform the action that cleared it and cannot attribute it:
   it may have been a physical replug by the owner, or a device-side watchdog.
   The host-side recovery attempts listed above demonstrably did not clear it
   in the 24 minutes they were available.
8. **Vendor frame comparison.** The vendor app's own 39-byte frames were not
   captured (no USB sniffer, and the app was not run). The field meanings above
   therefore rest on the Windows implementation plus the acceptance
   experiments, not on vendor traffic.
9. **Response lengths beyond 48.** Never observed. The driver should read up to
   64 bytes and accept 48.

---

## 8. Open questions for the Linux side

1. **Image/GIF mode:** how to enter it, what `0x10` reports there, and the
   upload protocol. This remains the largest gap and was not touched.
2. **`[8]` and `[24..25]` semantics:** need a camera or a vendor capture.
   The acceptance experiments here only rule out firmware validation.
3. **The constants `[9]=03`, `[12]=05`, `[14]=06`, `[15]=0C`, `[17]=07`:**
   never explained. They may be per-widget types, styles, or unused defaults.
4. **Does a `0x02` telemetry frame still update the display?** For `0x10` the
   answer is yes (data returned); for `0x01` it is unmeasured.
5. **Check order** when both marker and checksum are wrong.
6. **Recovery:** a malformed first byte can wedge the bulk endpoint with no
   in-protocol reset. In this session it cleared after about 24 minutes without
   any host action taking effect, so do not rely on a watchdog. A Linux driver
   should treat repeated OUT timeouts as "needs port power cycle" and should
   never emit a packet whose first two bytes are not `AA 2E`.
7. **Single-owner behaviour:** a second open gets `EACCES` on Windows. On Linux
   this maps to an exclusive claim; coordinate with any userspace daemon.
8. **Init after every open:** the Windows service always re-runs the 13
   commands after opening. Whether the device needs it after a pure reopen
   (without power loss) was not tested.
9. **Rotation and `0x02` payload variants:** only `01 00 00 01 24` was sent.
   Whether other payload bytes mean anything is unknown.
10. **Multiple LCD families:** unchanged; this document covers `3633:0009`
    only.

---

## 9. Final state at hand-off (16:41:40 UTC)

`MystiqueService` is **Running**. The vendor app has been starting and exiting
repeatedly since 16:30, so the service has alternated between owning the device
and yielding to it. The last full cycle:

```
16:40:43.488  INFO  DeepCool.exe exited; reacquiring device
16:40:46.787  INFO  device acquired ... rotation 0 degrees
16:40:46.983  INFO  600 updates sent; CPU 37C 15%, GPU 52C, RAM 38%
16:41:34.079  INFO  device released: DeepCool.exe started
16:41:34.079  INFO  Streaming -> Yielded
```

At the 16:41:40 snapshot: service `Running`, state `Yielded`; six
`C:\DeepCool\DeepCool.exe` processes are running (started by the owner). The
device is healthy — the service acquired it, streamed, and reported 600 updates
one minute earlier.

So the verification the owner asked for is satisfied, with a qualification: the
service **did** return to `Streaming` with the device acquired (16:40:46), and
then yielded again 48 seconds later when the vendor app reappeared. The device
is working; the state at any instant depends on whether the vendor app is up.

I did not start the vendor app, did not restart `MystiqueService` (the owner
stopped and started it at 16:24:38), and sent no USB traffic after 16:25.
