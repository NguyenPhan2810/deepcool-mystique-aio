# Design spec — Mystique Control

Shared input for three independent design directions. Each direction gets this file and nothing
else; they must not see each other's work.

## The product

**Mystique Control** is a desktop app (Rust + Tauri) that drives a DeepCool Mystique 360 AIO
liquid cooler's LCD panel over USB. The panel is a 2.8-inch 640×480 display on the pump head;
the app configures it and pushes telemetry to it.

The user owns exactly one of these, runs it on a homelab machine, and is migrating to Linux.
They are technical: comfortable with hex dumps, USB endpoints, and command bytes. They wrote the
original Windows service that this app replaces.

This is a **tool they will use weekly for years**, not a demo, not a landing page, not a
showcase. It should feel like equipment, not like marketing.

## What's on screen

Single page, 1440×900 desktop window. The real controls, in this content order:

**1. Connection**
- Device: `MYSTIQUE` · `3633:0009`
- Telemetry service: Running / Stopped
- Device handle: Open / Closed
- Actions: Open device, Close device, Stop service
- Reality: only one process can hold the device. When the service is running, the device
  cannot be opened. This conflict is the single most common failure, so it must read clearly.

**2. Brightness**
- Slider, 0–100, current value shown as a number
- Presets: 0, 25, 50, 75, 100

**3. Rotation**
- Four states: 0°, 90°, 180°, 270°
- Proven working on hardware
- Small note: rotation is ignored while the gyroscope is enabled

**4. Screen & gyro**
- Screen: On / Off
- Gyroscope: Allowed / Off

**5. Display areas** — two dropdowns
- Main Display Area: CPU Frequency, Time, Pump Speed, CPU FanSpeed,
  CPU FanSpeed + Pump Speed, Temperature
- Auxiliary Display Area: Voltage Mode, System Monitor, Core Data

**6. Media timing** — two dropdowns
- Image Switch Time: 3 s, 5 s, 7 s
- Image Switch Animation: Split Display, Scroll, Fade In/Out

**7. Labels** — two text inputs
- CPU name, e.g. `Intel Core i9-14900K`
- GPU name, e.g. `NVIDIA GeForce RTX 4070`
- These are pushed to the panel as ASCII, max 39 bytes. A character counter matters.

**8. Image upload**
- File drop / choose file
- Plan summary before sending: frames, total bytes, chunk count
- Upload button, with a confirm step
- Reality: uploads are slow and can fail silently; the plan summary exists so the user knows
  what they are about to send.

**9. Packet log**
- Scrolling list: `HH:MM:SS  cmd  payload-hex  → result`
- Real example lines:
  ```
  11:29:45  0x02  01 00 00 00 14  → 00
  11:29:46  0x01  24 00 00 06 00  → 00
  11:29:47  0x10  —              → 00
  ```
- This is the proof surface. When something does not work, this is where the user looks.

**Real telemetry to show as live state:** CPU 38 °C, GPU 52 °C, pump 2,780 rpm, brightness 70.

## Emotional tone

Precise, calm, trustworthy. The feeling of a well-made instrument: nothing decorative, nothing
hidden. Closer to a mixing console or a bench power supply than to a SaaS dashboard.

Not: playful, futuristic, neon, corporate, "AI product".

## Hard constraints

- **Format:** single-page, 1440×900. Must be legible at that size with no scrolling to reach
  brightness and rotation.
- **Readability floor:** body text ≥14px, labels and annotations ≥12px, body contrast ≥4.5:1.
  Whitespace must be composition, not absence.
- **Every element earns its place.** No decorative icons, no filler stats, no gradient slop,
  no emoji.
- **Numbering and units are part of the identity.** This is an instrument; how numbers are set
  matters more than how the container looks.
- **State must be visible at a glance:** what is connected, what is on, what is pending.
- **Failure must be visible in place.** An error appears in the section that caused it, not in
  a toast.

## Visual motif to consider

The panel itself is 640×480 with a fixed character — a small screen showing four numbers. The
app and the device show the same information at different scales. A direction that acknowledges
this — that the app is a larger, more precise view of a tiny screen — will feel more designed
than one that treats the app as a generic settings form.

## Deliverable

One self-contained HTML file per direction, plus a 1440×900 screenshot. Static is fine, but the
tabs/toggles should be clickable enough to read as a real interface.
