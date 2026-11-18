# Driver verification on hardware — first full pass

Date: 2026-11-18. App: `rust/apps/mystique-ui` (Tauri). Service stopped, device opened by the
app. The owner drove every control and watched the panel.

## Confirmed working from our own code

| Feature | Wire | Result |
|---|---|---|
| **Brightness** | `0x02[4]` | Works, 20 → 92 visibly dims and brightens |
| **Rotation** | `0x02[2]` | Works. Panel rotates. (UI failed to update its own label — frontend bug only) |
| **Gyroscope toggle** | `0x02[1]` | Works |
| **Main Display Area** | `0x04[0]` | Works — panel layout changed |
| **Image upload** | `0x0F` + 64-byte chunks | Works. A horizontal GIF uploaded in ~2 s and displayed |

The upload path, built from the USBPcap capture, is correct: header alone, JPEG chunks of 64
bytes, `dcldfinish`, commit. This is the first time the image path has worked from our code.

## Not working

### Screen on/off (`0x02[0]`) — wrong hypothesis

The toggle does nothing. But the owner found the answer by accident: **brightness 0 already
turns the screen off**. So "screen off" is a brightness value, not a separate enable bit.

`0x02[0]` is therefore *not* screen enable. It was only ever medium-confidence, and it is now
disproven as a screen control. Do not present it as one.

### Auxiliary Display Area (`0x04[1]`) — verified, but it is a *style* selector

Second pass, with telemetry running, owner observing:

| Value | Label | What the panel does |
|---|---|---|
| `00` | Voltage Mode | CPU frequency plus a voltage reading |
| `01` | System Monitor | the same, but the CPU frequency gains a ring |
| `02` | Core Data | the same, but drawn as a speedometer |

So the byte and the mapping are correct. Two corrections to the earlier assumption:

1. It is **not** "auxiliary data" — it selects a **presentation style** for the secondary area.
2. It only renders when Main Display Area is **CPU Frequency**. In the other five main layouts
   the value appears to be ignored, which is why the first test looked like a dead control.

This is verified on hardware from our own code.


## Missing capability, not a bug

**No telemetry stream.** The app sets layout and settings but never sends `0x01`, so the panel
shows a layout with no numbers. The Windows service did this; the Rust app does not yet. This
also plausibly explains the auxiliary field appearing dead.

## Upload quality gap

The image displays correctly but is **not cropped to the panel aspect**: a horizontal GIF is
letterboxed with blank bands top and bottom. The vendor app has a crop step before upload for
exactly this reason. Our uploader uses "contain"; it needs a "cover"/crop mode, and ideally a
preview of the crop.

## Frontend bug

Rotation works on the device but the selected-state label in the UI does not update after the
command succeeds. State is not being refreshed from the command outcome.

## Display mode — the answer was not a mode byte

After an upload the panel stays on the media content, and `0x03 [01]` + the 13-step handshake
did **not** bring it back. Verified on hardware:

> **Erasing the stored images (`0x14`) alone returned the panel to machine info.**

So "media mode" is not a mode flag at all — it is *content presence*. The device shows stored
media while any exists, and falls back to the dashboard when it does not. That also explains
why the panel survived a service restart and a full init: the content was still there.

Consequences:

- `0x03` is **not** the display mode. It toggled `01`/`02` around uploads in the capture, so it
  is more likely an image-vs-GIF or content-type selector. Its meaning is unresolved again.
- The app's `Monitor` button is misleading: it sends a byte that does nothing observable.
  "Return to dashboard" is **erase**, with the consequence stated plainly.
- **Erase is destructive.** It deletes whatever is stored. The UI must warn, not offer it as a
  casual mode toggle. The owner's own caution: "it might bite you."

This is the one place where the vendor-app capture misled us: the vendor sends `0x03` around
mode changes, so the correlation looked causal. It wasn't.

## What this pass establishes

Everything in the settings decode that has now been exercised is correct **except**
`0x02[0]`, which is disproven. The image protocol is correct as captured. The remaining gap
between this app and the vendor app is telemetry streaming and image cropping — both ordinary
software, not protocol unknowns.
