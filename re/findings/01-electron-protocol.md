# 01 — Electron/JS layer: IPC surface, display state machine, feature inventory

Scope: the **plaintext JavaScript** of DeepCool DeepCreative `1.2.12`
(`C:\DeepCool\resources\app.asar`), i.e. the renderer bundle and preload. The main
process is V8 bytecode; that is covered separately in
[`05-bytenode-protocol.md`](./05-bytenode-protocol.md). Neither document claims the
wire-level image-upload bytes — see "What still needs a live device" at the end.

All paths below are relative to the extraction at `re/work/asar/` (git-ignored).
Nothing here is redistributable vendor code beyond the short snippets needed as proof.

---

## 1. What the app does when it drives the panel

The UI is a Vue 3 SPA (`out/renderer/`) inside an Electron 23 shell. Each supported
device is a **route**, keyed by `productName`, and each route talks to the main process
through a **namespaced IPC channel family**. The main process is the thing that owns USB.

```
renderer (Vue)  --ipcRenderer.invoke("mystique/…")-->  main (bytecode)  --> USB
```

Renderer→main calls go through one helper, not raw `ipcRenderer.invoke`:

`out/renderer/assets/index-c58f7fdf.js:1150801-1150818`
```js
const ipcInstance = {
  send: (target, ...args) => {
    const payloads = args.map((e2) => toRaw(e2));
    return ipcRenderer.invoke(target.toString(), ...payloads).then((res) => {
      if (res.code) { Dialog.error(res.code, res.message); return Promise.reject(); }
      return Promise.resolve(res);
```
Two consequences for a reimplementation:

* every command is `invoke`-style (request/response), not fire-and-forget;
* **every reply is an envelope `{ code, message, data }`, and `code` truthy means
  failure** — `0`/absent is success. Error text is surfaced verbatim to the user.

Preload exposes exactly three objects and no generic `require` bridge —
`out/preload/index.js:5-9` (`invoke`, `on`, `removeAllListeners` only).

### Device discovery is event-driven, not polled by the renderer

`out/renderer/assets/index-c58f7fdf.js:42261` — `ipc.on("usb-change", (status) => …)`:
the main process pushes a hotplug event, the renderer re-fetches the device list
(`deviceStore.refresh()` → `app/get-device-list`). While on a *device detail* page, a
hotplug kicks the user back to the device list (`index-c58f7fdf.js:42263-42286`). The
route-name allow-list in that block is a de-facto family list:

`index-c58f7fdf.js:42263-42284`
```js
["LP","L094","mysitique","akdigitalpro","akdigital","ls","chdigital","ld-series","AG",
 "ASSASSIN4","ch510","chdigital360","GENOME-Ⅲ","L122","AK620G2","L086","L086Linker",
 "L136","C255","L142"]
```
Note `"mysitique"` — a vendor typo that also shows up in the channel list (§3).

---

## 2. Display-mode state machine

`DeviceInfo.mode` is a **nested object, `{ value }`**, and its value selects both the UI
panel and the device behaviour. Two independent tables agree:

`out/renderer/assets/index-366c4649.js:34463`
```js
computed(() => [null, _sfc_main$E, Motion, Recorder$1][props.device.mode.value])
```
`out/renderer/assets/index-366c4649.js:35086`
```js
return [null, Situational, Motion, Recorder][props.device.mode.value];
```

| `mode.value` | Meaning (UI) | i18n key | Renderer branch |
|---|---|---|---|
| `0` | no page / all-off | — | `index-366c4649.js:53300/53312/53318` show nothing |
| `1` | Monitor mode (machine info) | `devices.situationalMode`: **"Monitor Mode"** | `index-366c4649.js:53300` |
| `2` | Media mode (image + GIF) | `devices.motionmode`: **"Media Mode"** | `index-366c4649.js:53312` |
| `3` | Recording mode (on-screen recorder) | `devices.recorder`: **"Recording mode"** | `index-366c4649.js:53318` |

Mode 0 is what "screen off" looks like at the API level; separately there is a
`noLoadScreen` *idle* screen setting (§5).

### Media mode sub-state (image vs GIF, and slideshow)

`motionModeScreen = { type, index }` where **`type` `0` = image, `1` = GIF** and `index`
selects an item in the device's stored list:

`index-366c4649.js:53017-53041` (two identical handlers, one per list)
```js
await api.mystique.updateMotionModeScreen({ serialNumber,
  motionModeScreen: { index: index2, type: 0 } });   // type 1 for GIF
```
Confirmed by `index-366c4649.js:1241-1244`:
```js
const isJpgMode = computed(() => props.device.motionModeScreen.type === 0);
const isGifMode = computed(() => props.device.motionModeScreen.type === 1);
```

`motionMode = { animation, duration, gifRgbStatus, picRgbStatus }`
(`index-c58f7fdf.js:36378-36383`). `animation` is a 3-way radio —
`index-366c4649.js:52845-52922`, labels `label: 0` / `1` / `2`:

| `animation` | UI label | i18n key (EN) | Swiper effect / speed |
|---|---|---|---|
| `0` | Single image ("Split Display") | `devices.singlePic` | `"none"`, 0 ms |
| `1` | Scroll ("Panning") | `devices.flowPattern` | `"slide"`, 1000 ms |
| `2` | Fade in/out | `devices.fadeInOut` | `"fade"`, 1100 ms |

Effect/speed table: `index-366c4649.js:1239-1240`
```js
const swiperEffect = computed(() => ["none", "slide", "fade"][props.device.motionMode.animation]);
const swiperEffectSpeed = computed(() => [0, 1e3, 1100][props.device.motionMode.animation]);
```
`duration` = dwell time per image ("Image Switch Time", seconds — `devices.sec`).
When `animation` returns to 0, `duration` is forced to 0 and the index is re-derived
from the current item (`index-366c4649.js:53056-53064`).

> **No video mode for MYSTIQUE.** Video upload channels exist only for the
> L136/CH690 families (`c122/modifyVideo`, `c122/uploadSelectedVideo`,
> `ch690/modifyVideo`, `ch690/uploadSelectedVideo` — string pool of the main bundle,
> `re/work/index-strings.txt`). The MYSTIQUE page only ever renders image/GIF lists.

---

## 3. IPC table (renderer → main)

Channel names are proven string literals; payloads are proven from the call sites.
Arguments are positional; the envelope `{code,message,data}` comes back.

### `mystique/*` — the family this device uses

`out/renderer/assets/index-c58f7fdf.js:35515-35544`
```js
getDeviceInfo:            (serialNumber)        => send("mystique/get-device-info", serialNumber)
updateDeviceInfo:         (params)              => send("mystique/update-device-info", params)
updateMotionModeScreen:   (params)              => send("mystique/update-motion-mode-screen", params)
refreshRecorderMode:      (serialNumber)        => send("mystique/refresh-recorder-mode", serialNumber)
formatResources:          (serialNumber)        => send("mystique/format-resources", serialNumber)
uploadImage:              (params)              => send("mystique/upload-image", params)
resizeGif:                (localPath)           => send("mystique/resize-gif", localPath)
removeImage:              (serialNumber, id)    => send("mystique/remove-image", serialNumber, id)
```

| Channel | Args | Reply `data` | Proven at |
|---|---|---|---|
| `mystique/get-device-info` | `serialNumber: string` | full `DeviceInfo` (§4) | `index-c58f7fdf.js:36462-36478` |
| `mystique/update-device-info` | `DeviceInfo` | full `DeviceInfo` (echo) | `index-c58f7fdf.js:36551-36570` |
| `mystique/update-motion-mode-screen` | `{serialNumber, motionModeScreen:{type,index}}` | — | `index-366c4649.js:53017` |
| `mystique/refresh-recorder-mode` | `serialNumber` | — | `index-c58f7fdf.js:36499-36501` |
| `mystique/format-resources` | `serialNumber` | `{message}` | `index-366c4649.js:53068` |
| `mystique/upload-image` | `{id, path, x, y, width, height, model, rotate, serialNumber}` | `{message}` | `index-366c4649.js:52425-52434`, `53103-53106` |
| `mystique/resize-gif` | `localPath: string` | — | `index-c58f7fdf.js:35539` |
| `mystique/remove-image` | `serialNumber, id` | `{message}` | `index-366c4649.js:53092` |

`*` variants with a doubled slash (`mystique//update-motion-mode-screen`,
`mystique//get-device-screen-rorate`, `mystique/foramt-resources`) also exist in the
main-process string pool — almost certainly a second generation of the controller
(the pool also holds `MystiquePlusController` / `MystiquePlusUsbService`). **Inference**:
plus-variant channels, not used by this renderer.

### Support channels used by the MYSTIQUE page

| Channel | Purpose | Payload | Evidence |
|---|---|---|---|
| `sys/get-resources` | list stored images/GIFs | `serialNumber` | `index-c58f7fdf.js:36481` |
| `sys/select-image` | native open dialog | `extensions: string[]` | `index-366c4649.js:52487-52492` |
| `app/get-device-list` | enumerate devices | — | `index-c58f7fdf.js:36252` |
| `app/get-sensors-data` | live sensor snapshot | — | `index-c58f7fdf.js:1152226` |
| `app/get-fan-interface` | fan header choices | — | `index-366c4649.js` (`getFanInterface`) |
| `app/check-fw-update`, `app/update-firmware` | firmware | — | `index-c58f7fdf.js:1151800-1151900` |
| `worker/get-sdk-history`, `worker/clear-sdk-history` | recorder history | — | `index-c58f7fdf.js:35567-35568` |

### main → renderer (events, `ipcRenderer.on`)

| Event | Payload | Meaning | Evidence |
|---|---|---|---|
| `usb-change` | `status` | device attached/detached | `index-c58f7fdf.js:42261` |
| `worker/pic-change` | `latestIndex` | device advanced to next slideshow image | `index-366c4649.js:1326-1328` |
| `worker/rotate-change` | `serialNumber` | device was physically rotated; re-init | `index-c58f7fdf.js:42294` |
| `worker/rotate-focus-change` | `serialNumber` | rotation while the page was focused → modal warning | `index-c58f7fdf.js:42297-42309` |
| `worker/ready` | — | helper worker up | `index-c58f7fdf.js:42351` |
| `window-change` | `{ isMaximized }` | window state | `index-c58f7fdf.js:36306` |
| `app/progress`, `app/download-progress` | progress | updates | string pool |

`worker/pic-change` and the two `rotate-*` events are the only *device→UI* push paths
for the panel itself. Both are consistent with the panel having its own slide timer and
its own gyro: the app is notified rather than polling.

---

## 4. `DeviceInfo` — the exact object the main process accepts

`out/renderer/assets/index-c58f7fdf.js:36409-36444`
```js
DeviceData: {
  serialNumber: "",
  mode:              { value: 0 },
  options: {
    cpuFanIOInterface: "", gyroStatus: 0, noLoadScreen: 0, pumpFanIOInterface: "",
    rgbOptical: 0, screenBrightness: 0, screenRotate: 0, screenStatus: 0
  },
  situationalMode:   { auxiliaryAreaMode: 0, mainAreaMode: 0, spectrumType: 0, styleType: 0 },
  motionMode:        { animation: 0, duration: 0, gifRgbStatus: 0, picRgbStatus: 0 },
  motionModeScreen:  { type: 0, index: 0 },
  recorderMode:      { cpuClock: 0, cpuTemperature: 0 }
}
```
The store sends this object **whole** on every settings change
(`index-c58f7fdf.js:36551-36557`), i.e. the API is "write the entire config", not
per-field setters. A Linux implementation can mirror that.

`screenStatus` is the master screen on/off: the brightness slider is disabled whenever
it is falsy (`index-366c4649.js:35246-35247`).

---

## 5. Settings surface, with parameter ranges

Extracted from the page bundle + the English i18n block
(`index-c58f7fdf.js:30060-30130`).

| Setting | Field | Values | Evidence |
|---|---|---|---|
| Monitor / Media / Recording mode | `mode.value` | `0..3` | §2 |
| Idle screen | `options.noLoadScreen` | `0` = "Turn Off Screen Display", `1` = "Use Preset Animation (Recommended)" | `index-366c4649.js:35196-35205` |
| Screen on/off | `options.screenStatus` | `0`/`1` (switch `active-value:1`) | `index-366c4649.js:35236-35241` |
| Brightness | `options.screenBrightness` | slider, tooltip formatted as a percentage (`handlePercent`) | `index-366c4649.js:35243-35251` |
| Rotation | `options.screenRotate` | `0..3`, stepped +1 mod 4 by the UI | `index-c58f7fdf.js:36502-36510` |
| Auto-rotate (gyro) | `options.gyroStatus` | `0`/`1` | `index-366c4649.js:35275-35281` |
| RGB sync with lighting | `options.rgbOptical` | `1` = off, `2` = on ("temperature warning / GIF edge picking") | `index-366c4649.js:52938-52944` |
| CPU fan header | `options.cpuFanIOInterface` | string, from `app/get-fan-interface` | `index-366c4649.js:53218-53222` |
| Pump fan header | `options.pumpFanIOInterface` | string | same |
| Main display area | `situationalMode.mainAreaMode` | `0..5` → `[Core, System, Voltage, …]` (6 entries) | `index-366c4649.js:1184` |
| Aux display area | `situationalMode.auxiliaryAreaMode` | enum | `index-c58f7fdf.js:36372` |
| Style type | `situationalMode.styleType` | enum (Normal/Ring/Stopwatch) | i18n `devices.styleType` |
| Spectrum type | `situationalMode.spectrumType` | enum (Ring Wave / Bar Wave / Sound Wave) | i18n |
| Recorder content | `recorderMode.cpuClock`, `.cpuTemperature` | `0`/`1` each; at least one must stay on | `index-366c4649.js:53238-53250` |

Rotation semantics in the UI: `screenRotate ∈ {1,3}` is treated as landscape (4:3 crop,
"horizontal" widgets), `{0,2}` as portrait (3:4). See
`index-366c4649.js:52984` (`isHorizontal = screenRotate == 1 || == 3`) and the
`[1,3].includes(...screenRotate)` layout switches at `index-366c4649.js:74,142,210,…`.
This is a UI convention and does **not** by itself prove which value the panel's
`0x02` config byte expects for 0°/90°; the C# reference (`MystiquePackets.cs`) remains
authoritative for the wire value.

---

## 6. The image/GIF upload path, as far as plaintext goes

This is the important negative result: **the renderer never sends pixels.**

1. The user picks a file. The type filter is derived from the list being edited —
   images accept `["jpeg","jpg","jfif","png"]`, GIFs accept `["gif"]`
   (`index-366c4649.js:52488-52491`).
2. The file is shown in **Cropper.js** with a fixed aspect ratio per model
   (`index-366c4649.js:52394-52398`):
   ```js
   aspectRatio: {
     mystique: props.horizontal ? 4 / 3 : 3 / 4,
     "mystique-plus": 1
   }[props.model]
   ```
   The `model` prop is `productName.toLowerCase()` (`index-366c4649.js:53166`), i.e.
   `"mystique"` / `"mystique-plus"`.
3. On confirm it emits **a crop rectangle, not an image**:
   `index-366c4649.js:52425-52434`
   ```js
   return { id, path: localPath,
            x: Math.round(cropData.x), y: Math.round(cropData.y),
            width: Math.round(cropData.width), height: Math.round(cropData.height),
            model: props.model, rotate: cropData.rotate };
   ```
4. The page adds `serialNumber` and invokes `mystique/upload-image`
   (`index-366c4649.js:53078-53082`, `53103-53106`).

So the main process (bytecode) does: decode file → apply crop/rotate → scale to the
panel → encode/convey to USB. The renderer's contribution is the geometry only.

Related main-only string-pool symbols (names are proven; bodies are not):
`checkImageType`, `saveImage`, `saveGif`, `convertGifToJpeg`, `convertGifFirstFrameToJpeg`,
`mergeJpegToGif`, `getGifImageData`, `getImageFrameFromPlayerId`,
`removeImageToPlayer`, `normalizeForUpload`, `uploadChunk`, `batchUpload`,
`uploadBin`, `uploadGif`, `deviceRotateC`, `numToArr` — plus the native helpers
`resources/opencv/pixel.node` and `resources/magick/magick_x64.exe`, both loaded at
startup (probe, see doc 05).

⚠️ `uploadChunk` / `batchUpload` exist as *names in the main bundle*. That is
consistent with JS-side chunking of the image stream, but the chunk size is not
recoverable from the string pool. Treat as unresolved.

### The GIF "edge picking" flag

`gifRgbStatus` / `picRgbStatus` are separate from the UI switch `rgbOptical`, but the
switch writes `rgbOptical` (`2` on / `1` off, `index-366c4649.js:52938-52944`). The two
status fields are therefore device-reported state, read back via `getDeviceInfo`.
**Inference** — not proven, because the assignment happens in bytecode.

---

## 7. Resolution / pixel format / encoding — what the plaintext does and does not say

Proven:

* **Aspect ratios only.** `mystique` = 4:3 landscape / 3:4 portrait; `mystique-plus` = 1:1
  (`index-366c4649.js:52395-52398`).
* **No pixel-format or resolution constant exists anywhere in the renderer bundle.**
  I searched all `out/renderer/assets/*.js` for `320`, `240`, `480`, `360`, `x240`,
  `RGBA`, `BGRA`, `pixelFormat`; the only hits are echarts internals (colour tables,
  canvas sizing). Confirmed negative.
* Encoding on the wire is therefore chosen by the main process. The presence of
  `resources/opencv/pixel.node` (20 MB), `deep_cv_x64.node`, and a bundled ImageMagick
  binary means main does its own decode/scale/encode rather than shipping a raw canvas.

Inference (clearly marked): a 4:3 panel of this class is almost certainly **320×240**,
and the C# reference's observed 48-byte packet/39-byte payload budget is consistent with
a chunked upload rather than a single frame transfer. Neither number is provable from
the JS layer.

---

## 8. Addon ↔ device mapping (revised)

The original brief expected one native addon per family and asked which one serves
`3633:0009`. The answer from this layer plus the native-binary analysis is:
**none of them.**

Evidence from the JS layer:

1. **All six LCD addons are loaded unconditionally at main-process start**, before any
   device is known. Observed by running the real `out/main/index.jsc` under the app's
   own Electron runtime with `Module._load` instrumented — see doc 05 §3 for the log.
   They are therefore a fixed library, not selected per product.
2. The addons' exported APIs (extracted strings) are narrow and product-shaped:

   | Addon | Internal Rust module | Exports |
   |---|---|---|
   | `C122/index.node` | `napi_c122` | `refresh_device`, `close_device`, `send_image_data`, `send_general_command` |
   | `L086/index.node` | `napi_l086` | same four |
   | `L136/L136.node` | `napi_l086` | same four |
   | `CH690/CH690.node` | `napi_l136` | same four |
   | `L142/index.node` | `napi_l136` | same four |
   | `L122/index.node` | `napi_l122` | `refresh_device`, `get_response`, `start_up_animation`, `start_file_download`, `send_file_data`, `change_mode`, `change_settings`, `send_general_command`, `update_arguments`, `reboot`, `close_device` |

   (Source: `re/work/addon-strings/*.txt`, e.g. `L122_index.node.txt`:
   `sendImageDatacloseDevicerefreshDevicegetResponsesendGeneralCommandchangeModestartFileDownloadupdateArgumentsstartUpAnimationchangeSettingsreboot`.)
   Note `L136`'s binary is built from the `l086` source tree and `CH690`/`L142` from
   `l136` — the *filename* annotates the product, not the codebase.
3. All of them link **libusb via `rusb` 0.9.4** (paths to `rusb-0.9.4/src/*.rs` in every
   addon's strings), i.e. they are userspace USB, portable to Linux in principle.
4. The JS layer has a parallel, addon-free USB path: `usb` 2.14.0 is a runtime
   dependency (`package.json:56`), the main bundle's string pool contains `webusb`,
   `BaseDevice`, `checkSendResponse`, and the renderer reacts to a `usb-change` event.
   The `usb` package ships a libusb-backed WebUSB facade
   (`node_modules/usb/dist/webusb/webusb-device.js`, 17 KB, plain JS + source map) which
   is exactly the "libusb-over-WebUSB shim" shape.

**Conclusion (inference, strong):** the MYSTIQUE is driven by main-process JavaScript
through the `usb`/libusb layer with its own `BaseDevice`-style class, *not* through a
vendor `.node`. That is why no addon claims PID `0x0009`: the PID never reaches an
addon. This also explains why the protocol is expressible as plain bulk transfers with a
48-byte frame — the C# WinUSB client and this JS path are the same thing.

Counter-evidence I could not explain, recorded for the next agent: `AA 2E` *does* appear
as a deliberate 2-byte header write in two addons —
`C122/index.node` @ `0x12ef` and `L086/index.node` @ `0x2734`, both
`66 c7 00 aa 2e` (`mov word ptr [rax], 0x2eaa`) immediately after a 2-byte allocation.
Either those are a different family that shares the magic, or the native-binary survey
missed them. Worth 5 minutes with a disassembler; it does not change the conclusion
above because the PID match is absent.

---

## 9. What still needs a live device (or a decoded main bundle)

1. **Chunk size and pacing** for image/GIF upload — `uploadChunk` exists, its numeric
   arguments do not survive in plaintext.
2. **The mode-switch sequence around a transfer** (does upload require leaving
   monitor mode?) — likely `mystique/upload-image` → `change_mode`-equivalent.
3. **Pixel format** actually sent (RGB565? JPEG? raw RGBA?) — main-side only.
4. **Panel native resolution** — 4:3 aspect is proven, the number is not.
5. **GIF storage model**: the device exposes a *list* with indices
   (`sys/get-resources` → `{jpg:[…], gif:[…]}`, `motionModeScreen.index`), plus
   `format-resources`. Is the panel storing compressed files in its own flash, or does
   the host re-upload on every switch? A `format` + `index` API strongly suggests
   device-side storage. **Inference**; confirm by uploading two images and power-cycling.
6. **`checkSendResponse` semantics** — retry/ack rules. Bytecode-only.

---

## Appendix — where each claim came from

| Claim | File:line |
|---|---|
| IPC envelope `{code,message,data}` | `out/renderer/assets/index-c58f7fdf.js:1150801-1150818` |
| `mystique/*` channel list | `index-c58f7fdf.js:35515-35544` |
| DeviceInfo shape | `index-c58f7fdf.js:36409-36444` |
| Mode table 0..3 | `index-366c4649.js:34463`, `35086`, `53300/53312/53318` |
| `motionModeScreen.type` 0=image 1=gif | `index-366c4649.js:1241-1244`, `53017-53041` |
| Animation 0/1/2 | `index-366c4649.js:1239-1240`, `52845-52922` |
| Upload payload (crop rect) | `index-366c4649.js:52425-52434` |
| 4:3 / 3:4 / 1:1 aspect | `index-366c4649.js:52395-52398` |
| `noLoadScreen` 0/1 | `index-366c4649.js:35196-35205` |
| Rotate 0..3 stepping | `index-c58f7fdf.js:36502-36510` |
| `rgbOptical` 1/2 | `index-366c4649.js:52938-52944` |
| Hotplug + family route list | `index-c58f7fdf.js:42261-42286` |
| Addon export names | `re/work/addon-strings/*.txt` |
| `usb` version | `package.json:56` (`"usb": "2.14.0"`) |
