# 02 — Native addons & services: static reverse engineering

Scope: the compiled `.node` N-API addons and the three service executables shipped
with the DeepCool app. Read-only against `C:\DeepCool`. No execution of any target.
No firmware touched.

Everything below is either **(observed)** — a byte, string, import, or instruction I
read out of the file — or **(inference)** — my reasoning on top of observations.

---

## 0. Headline

1. **No native addon serves `3633:0009`.** All seven LCD addons match vendor ID
   `0x3633` and exactly one *other* product ID each (`0x22`, `0x26`, `0x27`, `0x2E`,
   `0x30`, `0x31/0x32`). The literal constant `0x0009` never appears as a comparison
   in any of them.
2. **The MYSTIQUE wire marker `HIDC` appears in no native binary** (and in the asar
   only as C++ mangled `HID::HID` symbols). So the `AA 2E … HIDC` protocol is not
   implemented in native code at all in this build.
3. All seven LCD addons **statically link libusb 1.x** and talk USB directly — they do
   *not* go through `deep_service.exe`. Whichever family applies, the panel is owned by
   the Electron process, not by a background service.
4. `DeepCoolDisplayService.exe` **never opens the LCD panel**. It creates *virtual*
   (indirect) displays and IOCTLs its own software device. It is not a USB owner.

The practical consequence for the Linux driver: this installed app build is **not** a
usable source of the MYSTIQUE command table. The protocol knowledge has to come from
the repo's existing C# reference plus live capture — or from decoding the renderer's
V8 bytecode (see §7).

---

## 1. Per-addon inventory

All targets are **PE32+ , machine `0x8664` (x86-64)**, DLL subsystem (`subsystem = 2`),
single export `napi_register_module_v1`. Section entropies are Shannon bits/byte over
the raw section bytes.

| File | Bytes | Internal Rust module | Accepted VID:PID | USB stack | Talks to |
|---|---|---|---|---|---|
| `C122\index.node` | 363 008 | `napi_c122` | `3633:0022` | libusb (static) | panel, direct |
| `CH690\CH690.node` | 808 448 | **`napi_l136`** | `3633:0030` | libusb (static) | panel, direct |
| `L086\index.node` | 371 712 | `napi_l086` | `3633:0027` | libusb (static) | panel, direct |
| `L122\index.node` | 422 400 | `napi_l122` | `3633:0026` | libusb (static) | panel, direct |
| `L122\index_back.node` | 421 376 | `napi_l122` | `3633:0026` | libusb (static) | panel, direct |
| `L136\L136.node` | 808 448 | **`napi_l086`** | `3633:002E` | libusb (static) | panel, direct |
| `L142\index.node` | 808 448 | **`napi_l136`** | `3633:0031` **and** `3633:0032` | libusb (static) | panel, direct |
| `capture\wincapture-node.node` | 34 114 560 | (C++/embind) | none | D3D11/DXGI + SetupAPI | screen capture, no USB |

Section entropy and SHA-256 (first 16 hex):

| File | .text | .rdata | .data | .pdata | .reloc | sha256[:16] |
|---|---|---|---|---|---|---|
| `C122\index.node` | 6.29 | 5.54 | 1.99 | 5.63 | 4.67 | `82c2a628f3d0cc91` |
| `CH690\CH690.node` | 5.78 | 5.18 | 4.78 | 5.83 | 4.97 | `5571e8f5646b2be4` |
| `L086\index.node` | 6.29 | 5.55 | 2.00 | 5.60 | 4.74 | `c038b092c2ac82ad` |
| `L122\index.node` | 6.27 | 5.50 | 2.15 | 5.62 | 4.91 | `dcf6733d776551f8` |
| `L122\index_back.node` | 6.28 | 5.51 | 2.19 | 5.56 | 4.93 | `8ca329ebb183969c` |
| `L136\L136.node` | 5.78 | 5.17 | 4.78 | 5.84 | 4.97 | `50ab7fdb77efe284` |
| `L142\index.node` | 5.78 | 5.18 | 4.78 | 5.83 | 4.97 | `d7c66c8b7df3db2e` |

No packing or encryption: `.text` entropy 5.78–6.29 is normal for compiled Rust, and
`.rdata` is plainly readable (all library strings below are in the clear).

### 1.1 Imports — the decisive shape

Every LCD addon imports the **same** small set and nothing else:

```
kernel32.dll (75-77)   VCRUNTIME140.dll  ntdll.dll  bcryptprimitives.dll
api-ms-win-core-synch-l1-2-0.dll
api-ms-win-crt-{math,string,runtime,heap,convert,environment,stdio}-*.dll
```

Interfaces actually used from `kernel32` (union over all seven; the intersection is
identical minus `GetConsoleOutputCP`/`IsWow64Process`):

```
CancelIoEx  CreateFileA  CreateIoCompletionPort  DeviceIoControl  GetOverlappedResult
GetQueuedCompletionStatus  LoadLibraryA  PostQueuedCompletionStatus  ReadFile  WriteFile
CreateEventA  CreateWaitableTimerA  SetWaitableTimer  WaitForMultipleObjects
GetSystemDirectoryA  GetVersionExA  FormatMessageA  QueryPerformanceCounter
```

**There is no `SETUPAPI.dll`, no `hid.dll`, no `winusb.dll`, and no `Cfgmgr32.dll`
import in any addon.** That is not an oversight — it is exactly the libusb-on-Windows
runtime-loading pattern, confirmed by these strings in every addon's `.rdata`:

- `https://libusb.info`
- `libusb_get_device_list`, `libusb_unref_device`, `libusb_open`, `libusb_close`,
  `libusb_claim_interface`, `libusb_release_interface`, `libusb_init_context`,
  `libusb_control_transfer`, `libusb_submit_transfer`, `libusb_cancel_transfer`
- `libusb v%u.%u.%u.%u%s`
- Late-bound backend strings: `SetupAPI` / `SetupDiGetClassDevsA` /
  `SetupDiEnumDeviceInfo` / `SetupDiGetDeviceInterfaceDetailA` / `SetupDiDestroyDeviceInfoList`,
  `WinUSB` / `WinUsb_Initialize` / `WinUsb_WritePipe` / `WinUsb_ReadPipe` /
  `WinUsb_SetPipePolicy`, `libusbK`, `UsbDk`, `HID API`, `Composite API`
  (these are libusb's own `load_system_library` + `GetProcAddress` descriptors, which is
  why `setupapi` never shows up in the import table)

Rust provenance is visible and useful for attribution:

| Addon | registry path in `.rdata` | crate |
|---|---|---|
| C122, L086, L122, L122_back | `C:\Users\admin\.cargo\registry\src\mirrors.ustc.edu.cn-*\…` | `rusb-0.9.4`, `libusb1-sys-0.7.0`, `neon-1.0.0` |
| CH690, L136, L142 | `C:\Users\Luna\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\rusb-0.9.4\src\context.rs` | `rusb-0.9.4` |

Two different build machines (`admin`, `Luna`) and two different crates.io mirrors. That
matches the size split: the three 808 448-byte binaries are one build lineage, the rest
another **(inference)**.

They are *not*, however, byte-identical with a patched constant — do not assume a simple
one-field patch. Whole-file byte diff against `L142\index.node`:

```
L136\L136.node    same length  diff bytes 654 591   first diff at 0x108
CH690\CH690.node  same length  diff bytes 526 300   first diff at 0x108
```

Same crate, same size, same section profile, materially different code layout — i.e. three
separate compilations of the same source, not one binary shipped three times.

### 1.2 Internal module names are misleading

Observed backtrace-style symbol strings in `.rdata`:

```
C122\index.node   : napi_c122::refresh_device  close_device  send_image_data  send_general_command
CH690\CH690.node  : napi_l136::send_image_data close_device  refresh_device  send_general_command
L086\index.node   : napi_l086::close_device    refresh_device  send_general_command  send_image_data
L122\index.node   : napi_l122::refresh_device  get_response  start_up_animation  start_file_download
                    reboot  change_settings  send_general_command  update_arguments
                    send_file_data  change_mode  close_device
L136\L136.node    : napi_l086::close_device    send_image_data  refresh_device  send_general_command
L142\index.node   : napi_l136::send_image_data close_device  refresh_device  send_general_command
```

So **`CH690.node` and `L142\index.node` are both built from the `napi_l136` crate**, and
**`L136\L136.node` is built from the `napi_l086` crate**. Three of seven directory names
do not match the code inside them. Directory naming is not evidence; the PID constants are.

---

## 2. Addon-to-device mapping verdict

### 2.1 Method

For each binary I disassembled `.text` with `llvm-objdump -d --no-show-raw-insn` and
searched for the immediate `0x3633`:

```
& "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\Llvm\x64\bin\llvm-objdump.exe" -d --no-show-raw-insn <file> > re\work\pe\_disasm_<name>.txt
grep '0x3633' _disasm_*.txt
```

Result: **exactly one hit per binary**, and in every case the very next comparison is the
product ID. The pattern is the `rusb` enumeration idiom
(`for dev in list.iter() { let d = dev.device_descriptor()?; if d.vendor_id() == V && d.product_id() == P { … } }`).

### 2.2 Evidence, one binary at a time

**C122** — `C122\index.node`, `.text` VA `0x180005ff6`:
```
180005fd2:  movzwl 0x78(%rbp), %edx        ; descriptor.idVendor
180005fd6:  movzwl 0x7a(%rbp), %eax        ; descriptor.idProduct
180005ff6:  cmpl   $0x3633, %edx
180005ffc:  jne    0x180005f80
180005ffe:  cmpw   $0x22, %ax
180006002:  jne    0x180005f80
```
→ `3633:0022`

**CH690** — `CH690\CH690.node`, VA `0x180003c28`:
```
180003c19:  callq  …              ; rusb DeviceHandle/descriptor accessor
180003c28:  cmpw   $0x3633, %ax
180003c2c:  jne    0x180003c40
180003c35:  callq  …              ; product_id()
180003c55:  cmpw   $0x30, %ax
```
→ `3633:0030`

**L086** — `L086\index.node`, VA `0x1800051c1`:
```
1800051c1:  cmpw   $0x3633, 0x78(%rbp)
1800051c7:  jne    0x180005181
1800051c9:  cmpw   $0x27, 0x7a(%rbp)
1800051ce:  jne    0x180005181
```
→ `3633:0027`

**L122** — `L122\index.node`, VA `0x180001441`:
```
180001441:  cmpw   $0x3633, 0x78(%rbp)
180001447:  jne    0x180001401
180001449:  cmpw   $0x26, 0x7a(%rbp)
18000144e:  jne    0x180001401
```
→ `3633:0026`

**L122_back** — `L122\index_back.node`, VA `0x1800013f1`: identical shape,
`cmpw $0x3633, 0x78(%rbp)` then `cmpw $0x26, 0x7a(%rbp)` → `3633:0026`.

**L136** — `L136\L136.node`, VA `0x1800091f8`:
```
1800091f8:  cmpw   $0x3633, %ax
1800091fc:  jne    0x180009210
180009225:  cmpw   $0x2e, %ax
180009229:  jne    0x180009210
```
→ `3633:002E`

**L142** — `L142\index.node`, VA `0x180003c28`: accepts **two** product IDs:
```
180003c28:  cmpw   $0x3633, %ax     ; vendor
180003c55:  cmpw   $0x31, %ax       ; product 0x31
180003c69:  callq  0x180009bb0
180003c86:  cmpw   $0x32, %ax       ; else product 0x32
180003c8a:  je     0x180003c5b
```
→ `3633:0031` **or** `3633:0032`

### 2.3 Negative result — PID `0x0009`

```
grep -c 'cmpw\s+\$0x9,' _disasm_*.txt     →  0 in all seven files
grep -c '0x3633'          _disasm_*.txt   →  1 in each file (no second matcher)
```

There is no second device-matching path: a monomorphised `rusb` matcher would emit a
second `$0x3633` comparison, and there is none. The accepted PID is not read from
JavaScript either — the value is a compiled immediate in every case.

Serially the family table is:

| PID | addon | JS model-config instance |
|---|---|---|
| `0x0022` | C122 | `c122ModelConfigInstance` |
| `0x0026` | L122 / L122_back | `l122ModelConfigInstance` |
| `0x0027` | L086 | `l086ModelConfigInstance` |
| `0x002E` | L136 | `l136ModelConfigInstance` |
| `0x0030` | CH690 | `ch690ModelConfigInstance` |
| `0x0031`, `0x0032` | L142 | `l142ModelConfigInstance` |
| `0x0009` | **— none —** | — |

The JS side confirms the six-and-only-six shape: the asar contains exactly one
`init…Player` per family — `initC122Player`, `initL086Player`, `initL122Player`,
`initL136Player`, `initL142Player`, and (for CH690) a `ch690ModelConfigInstance` with no
separate player. Six families, seven files (L122 has a v1/v2 pair). **There is no seventh
family left over that could own PID `0x0009`.**

### 2.4 Verdict

> **No shipped native addon serves VID `3633` / PID `0009`.**
>
> The MYSTIQUE panel as enumerated on this machine (`USB\VID_3633&PID_0009\…`) is not the
> device any of these binaries opens. The addons cover the `0x22/0x26/0x27/0x2E/0x30/0x31/0x32`
> family; `0x0009` is outside that set.

Two candidate explanations, both marked **(inference)**:

- **(a)** The MYSTIQUE transport in this app build lives in the renderer's JavaScript, not
  in a `.node`. Supporting evidence: the renderer bundle contains WebUSB
  (`navigator.usb`, `requestDevice`, `transferIn`, `transferOut`, `claimInterface`) plus a
  libusb-over-WebUSB shim with the comment *"C++ equivalent of `await navigator.usb.getDevices()`"*,
  and `navigator.usb` is only reachable from a renderer. A renderer-side driver would
  explain the total absence of `HIDC` from every native module.
- **(b)** This app build (files dated 2026-04-22) simply does not drive the plain MYSTIQUE
  panel; it covers MYSTIQUE-PLUS-class hardware under PIDs `0x31`/`0x32`.

Neither is proven by native code alone. What *is* proven is the negative: **do not look
for the `HIDC` protocol in these DLLs.** It is not there.

---

## 3. "MYSTIQUE addon" — what can and cannot be recovered

There is no MYSTIQUE addon, so there is no MYSTIQUE command table to recover from native
code. For completeness, the two things I *can* state statically:

### 3.1 `HIDC` is absent from native code — with the false positive explained

Raw byte search for the ASCII sequence `HIDC` across all eleven native targets
(all 7 LCD addons, `wincapture-node.node`, `deep_service.exe`,
`DeepCoolDisplayService.exe`, `SensorBridgeServer.exe`): **zero hits**.

In `app.asar` the sequence occurs 23 times, and every one is a false positive:

```
_ZN3HIDC1ERKN4Napi12CallbackInfoE   → HID::HID(Napi::CallbackInfo const&)      [C1 = complete ctor]
_ZN3HIDC2ERKN4Napi12CallbackInfoE   → HID::HID(Napi::CallbackInfo const&)      [C2 = base ctor]
HIDAPI_HIDCLASS_H                   → include guard in the bundled hidapi header
```

That is the **node-hid** addon (`hid_open_path` ×63, `HIDAsync` ×1173 in the asar), used
by other DeepCool product families. It is not the `HIDC` request trailer.

**Consequence:** the `AA 2E CMD … "HIDC" sum16` framing documented in `CONTEXT.md` has no
counterpart in any shipped binary. The repo's C# implementation is currently the only
known-good description of it.

### 3.2 The `L122` method surface — the richest protocol-shaped API available

`L122\index.node` is the only addon that exposes more than four methods. Its registered
names (from `napi_l122::` symbol strings at `.rdata` file offset `0x4844c`):

```
refreshDevice  getResponse  startUpAnimation  startFileDownload  reboot
changeSettings  sendGeneralCommand  updateArguments  sendFileData  changeMode  closeDevice
```

That vocabulary implies a real protocol with: a mode switch, a settings/arguments update,
a *file download session* (start → data chunks → implicit end), an upload of stored
animation, and a reboot. It is a different, richer protocol than the four-verb
`sendImageData / sendGeneralCommand / refreshDevice / closeDevice` surface of the other six.

**If the goal is "a panel protocol we can actually read statically", `L122` is the best
target** — but it is `3633:0026`, not the MYSTIQUE.

### 3.3 Buffers, chunking, retries in the MYSTIQUE path

Not statically recoverable, because the code is not present (§2.3, §3.1). What the
binaries do tell us about the *other* family, stated as observation:

- `C122` has explicit upload-phase error strings — `send start command failed`,
  `send file command failed`, `send finish command failed`, and a `src\usb.rs` module path
  (`.rdata` offset `0x3d778`/`0x3d6d8`). **(inference)** this is a three-phase upload
  (start → data → finish), matching the `startFileDownload`/`sendFileData` naming in L122.
- The renderer bytecode contains `uploadChunk`, `batchUpload`, `-chunk-start`,
  `Transfer CRC error`, `Transfer sequence error:`, `Timeout retry:`, `Transfer (IN|OUT) Timeout`
  (`app.asar` offset `0x12d4cda`). **(inference)** at least one DeepCool display protocol
  is chunked, CRC-checked, sequence-numbered, and retried on timeout. That is a *different*
  framing from `CONTEXT.md`'s plain 16-bit sum.

I am deliberately not assigning chunk sizes: every candidate (48/64/512/1024) would be a
guess, and the layout that would confirm it is in bytecode, not in these DLLs.

---

## 4. How the addons are driven from JS

All seven export the single standard entry point, so the surface is a plain object of
functions:

| Export from the `.node` | Kind |
|---|---|
| `napi_register_module_v1` | only PE export, ordinal 1, `rva 0x8010` (C122) … `0x5350` (L142) |

Registered methods (observed as contiguous `.rdata` name clusters, in registration order):

| Addon | Methods |
|---|---|
| C122 | `refreshDevice`, `closeDevice`, `sendImageData`, `sendGeneralCommand` |
| L086 | `closeDevice`, `refreshDevice`, `sendGeneralCommand`, `sendImageData` |
| L136 / CH690 / L142 | `sendImageData`, `closeDevice`, `refreshDevice`, `sendGeneralCommand` |
| L122 / L122_back | `refreshDevice`, `getResponse`, `startUpAnimation`, `startFileDownload`, `reboot`, `changeSettings`, `sendGeneralCommand`, `updateArguments`, `sendFileData`, `changeMode`, `closeDevice` |

Argument and return shapes, as far as they are statically constrained:

- **`refreshDevice`** — the enumerator/opener. It is the function containing the
  `$0x3633` comparison (§2.2), so it takes **no VID/PID arguments**: the pair is compiled
  in. It walks `libusb_get_device_list`, matches, opens, claims an interface.
  **(inference)** returns a handle/index or `undefined` on failure; failures surface as the
  Rust strings `device not found` (C122 `.rdata` `0x3e0d8`) and
  `Can't init Global usb context, error ` (`.rdata` `0x40448`).
- **`closeDevice`** — no arguments; releases the interface and closes the handle.
- **`sendImageData`** — the bulk path; the C122 strings `send start command failed` /
  `send file command failed` / `send finish command failed` place the phase errors inside
  this call. **(inference)** accepts a `Buffer` of already-encoded image bytes plus a small
  numeric descriptor. It does **not** encode images: no JPEG/PNG/GIF/zlib code is linked
  into any addon. Verified — searching all seven addons for
  `jpeg|jpg|libpng|deflate|GIF89|IDAT|zlib` returns 5 hits each, and all 5 are `.idata$2`
  … `.idata$6` COFF section-name fragments (the substring `dat` in `idata`). Zero real codec
  symbols. Encoding happens on the JS side or in the external `magick`/`ffmpeg` binaries.
- **`sendGeneralCommand`** — the control path used for brightness/rotation/settings
  (the renderer's `BrightnessControl` → `updateSettingWithParam` → `sendGeneralCommand`
  chain is visible in the bytecode region).
- **L122 only**: `startFileDownload` → repeated `sendFileData` → done, with
  `changeMode` / `changeSettings` / `updateArguments` / `reboot` / `startUpAnimation` as
  discrete commands and `getResponse` as a synchronous read.

Return shapes are N-API `undefined` / `string` for the simple four, and the Rust `Result`
mapping in the CRTs suggests `string | undefined` errors (`failed` / `success` literals at
C122 `.rdata` `0x3e180`).

---

## 5. `DeepCoolDisplayService.exe` verdict

**It does not open the LCD USB device. It creates indirect (virtual) displays and
IOCTLs its own software device.**

Evidence:

- **Imports** (`pe\DeepCoolDisplayService.json`): `KERNEL32` (139), `ADVAPI32` (5),
  `SETUPAPI` (13), `api-ms-win-devices-swdevice-l1-1-0` (2). **No `libusb`, no `WinUsb_*`,
  no `HidD_*`, no `hidapi`** — string counts are zero for each.
- **The SetupAPI set is driver-installation, not device access**:
  `SetupCopyOEMInfW`, `SetupIterateCabinetW`, `SetupDiBuildDriverInfoList`,
  `SetupDiGetDriverInfoDetailW`, `SetupDiEnumDriverInfoW`, `SetupUninstallOEMInfW`,
  `CM_Uninstall_DevNode`, `CM_Locate_DevNodeW`.
- **The device it opens is its own software node**: `SwDeviceCreate` / `SwDeviceClose`
  (from `api-ms-win-devices-swdevice`) followed by `SetupDiEnumDeviceInterfaces` +
  `SetupDiGetDeviceInterfaceDetailW` + `DeviceIoControl`.
- **The IOCTLs name display creation, and take a serial, not pixels**:
  ```
  Sending IOCTL_IDD_CREATE_L136_DISPLAY: deviceSn {}, width {}, height {}, refresh {}
  Sending IOCTL_IDD_CREATE_CH690_DISPLAY: deviceSn {}, width {}, height {}, refresh {}
  Sending IOCTL_IDD_DESTROY_L136_DISPLAY...
  ```
  (`deviceSn` is a tag, `width`/`height`/`refresh` are the virtual monitor's geometry.
  No frame buffer is sent.)
- **Its IPC is a named pipe, not USB**: `\\.\pipe\display_data`, `DataPipeName`,
  `\\.\pipe\`, `Global\DeepCool-Display-Service-Ready`, `DeepCool_display_server`,
  `display server waitting`.
- **It ships the driver payload**: `cab\DeepCoolDisplayDriver.dll`,
  `cab\DeepCoolDisplayDriver.inf`, and PDB path
  `E:\project\mine\DeepCoolDisplayDriver\x64\Release\DeepCoolDisplayService.pdb`.

So the architecture is: *render to a virtual monitor created by this service → capture
the monitor → push the pixels over USB elsewhere.* This service is the **first** half.

### The other two — for completeness

| Exe | Imports | Role **(observed strings + imports)** |
|---|---|---|
| `deep_service.exe` ("Deep Cool Helper Service") | `SETUPAPI` (5): `SetupDiGetClassDevsW`, `SetupDiEnumDeviceInfo`, `SetupDiDestroyDeviceInfoList`, `CM_Get_Device_IDW`, `CM_Request_Device_EjectW`; `libczmq.dll`, cJSON | **USB hotplug / safe-eject helper.** Strings `USB\VID_%04X&PID_%04X`, `hotPlugUSBDevice`, `DeviceIoControl failed with error code`. The `%04X` format proves the VID/PID is *supplied at runtime* (over ZeroMQ), not compiled in — so even this exe cannot tell us what PID the app uses. |
| `SensorBridgeServer.exe` | `wlanapi`, `bthprops.cpl`, `IPHLPAPI`, `ole32` | Sensor/WMI/network/Bluetooth collector published on the `deepcool_sensor_data` pipe. No USB. |

Nobody but the Electron process touches the panel.

**Linux implication (inference):** there is no Linux equivalent of the IDD half, and none
is needed for *our* goal — we want the panel, and on Linux we will render offscreen
ourselves. The absence of an IDD on Linux does not block a panel driver; it only means we
cannot reuse DeepCool's "virtual monitor + capture" pipeline.

---

## 6. Tooling limitations actually hit

Recorded honestly, because the task brief assumed `rea inspect` works engine-free on PE.
It does not, in this installation.

| Attempt | Result |
|---|---|
| `rea doctor` | `healthy: false`; ghidra `not_configured` (`GHIDRA_INSTALL_DIR` unset), ida `not_configured`, hopper `unsupported_host`, kuna `not_configured`, jadx `not_configured` |
| `rea capabilities` | `analysis_provider_binding: null`; no native analysis provider available |
| `rea inspect <file> --json` | **fails**: `provider_unavailable`, `candidates: [ghidra, hopper, ida, kuna]`, all rejected. Saved at `re\work\rea\<name>.json` (3 060 bytes each, identical error) |
| `rea inspect-binary-layout <file> --json` | **fails**: `capability_unavailable` — *"The initial real-verified pwntools artifact profile supports Linux x64 only."* (`provider_id: pwntools-elf`). Saved at `re\work\rea\layout-<name>.json` |
| `rea inspect-with-objdump <file>` | **fails**: `Unable to run configured command (ERR_REA_WINDOWS_NATIVE)` — it wants GNU `objdump`, which is not installed |
| `rea inspect-pe-resources <file>` | **works** (provider `rea-pe-resources`). Gives sha256, `format: pe32-plus`, `machine: 34404`. Used to cross-check hashes. |
| `rea setup` | **not run** (prohibited) |

What I used instead — all engine-free and sufficient for the conclusions above:

- `pefile` 2024.8.26 (Python 3.12.10): sections, entropy, imports, delay imports, exports,
  data directories → `re\work\pe\*.json`, script `re\work\pe_inventory.py`.
- Custom strings extractor (ASCII ≥4 + UTF-16LE ≥4, with file offsets) →
  `re\work\strings\*.jsonl` / `*.txt`, script `re\work\extract_strings.py`.
- `llvm-objdump.exe` from VS 2022 18 Community
  (`…\VC\Tools\Llvm\x64\bin\llvm-objdump.exe`) for full `.text` disassembly →
  `re\work\pe\_disasm_*.txt`, script `re\work\disasm_all.py`.
- `dumpbin.exe` is also present (`…\VC\Tools\MSVC\14.44.35207\bin\HostX64\x64\dumpbin.exe`)
  but was not needed.
- `capstone` is **not installed** (`pip show capstone` → not found). I did not install it.

### Exact next step that would unblock deeper analysis

**To decompile (the only thing still blocking):** supply a Ghidra 12.1.x extraction and set
`GHIDRA_INSTALL_DIR` — that is a single environment variable, no `rea setup`, no config
rewrite:

```powershell
$env:GHIDRA_INSTALL_DIR = "C:\tools\ghidra_12.1_PUBLIC"
rea doctor            # ghidra should flip to available
rea inspect "C:\DeepCool\resources\app.asar.unpacked\resources\L142\index.node" --format json
```

Note `rea` also needs an absolute `java.exe` path on this host — the doctor currently
reports `java.exe: Owned Windows process requires an absolute executable path [java.exe] (Win32 87)`,
so `JAVA_HOME` must be set too. Without that, Ghidra will still report `version_unresolved`.

**But decompilation is not on the critical path for MYSTIQUE.** The panel protocol is not in
these binaries (§2.3, §3.1). The step that actually unblocks the Linux driver is:

1. **Decode the renderer bytecode.** The app's own JS is bytenode-compiled V8 bytecode.
   Evidence it is bytecode and not text: at `app.asar` offset `0x130299c` the token
   `refreshDevice` is surrounded by opcode bytes and V8 register annotations
   (`$Qg2/`, `RbJ`, `Ld`, `Ke&`), and sibling strings read
   `c122ModelConfigInstance`, `initC122Player`. Numeric literals in bytecode are binary
   tagged values, which is exactly why `grep 13875` and `grep 0x3633` find nothing — the
   device table is there as numbers, not text. Decoding the bytecode for the
   `BaseDevice` class (visible at `0x12d4cda` alongside `checkSendResponse`,
   `checkReadResponse`, `sendTask`, `uploadChunk`, `batchUpload`) will yield the
   VID/PID filters and, if a MYSTIQUE path exists, its `AA 2E … HIDC` builder with the
   command bytes as constants.
2. **In parallel, capture the wire.** `MystiqueDevice` already drives the panel from
   Windows; a USBPcap-free capture can be taken by instrumenting that code path, which is
   lower-risk than fighting a bytecode VM. This is the fastest route to the image/GIF
   upload framing that `CONTEXT.md` lists as the main gap.

---

## 7. Files produced

```
re\work\pe_inventory.py                  PE inventory script
re\work\extract_strings.py               strings extractor
re\work\disasm_all.py                    llvm-objdump driver
re\work\find_pids.py                     VID/PID immediate enumerator
re\work\asar_vidpid.py                   asar byte scan helper
re\work\asar_glue.py                     asar N-API-name search helper
re\work\run_inspect.ps1                  (failed) rea inspect sweep — kept as evidence
re\work\run_layout.ps1                   (failed) rea inspect-binary-layout sweep
re\work\pe\<name>.json                   per-target PE facts
re\work\pe\_disasm_<name>.txt            full .text disassembly
re\work\pe\_summary.json                 inventory summary
re\work\strings\<name>.jsonl / .txt      strings with offsets
re\work\rea\<name>.json                  rea failure records (provider_unavailable)
re\work\rea\layout-<name>.json           rea failure records (capability_unavailable)
```

No file outside `re\` was created or modified. `resources\fw\` was never opened.
