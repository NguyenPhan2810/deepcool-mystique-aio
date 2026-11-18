# 20 — Stream rate: what actually limited it to 1 Hz

Measured 2026-11-18 on the 360, `3633:0009`, Windows, `MystiqueService` stopped.

Short version: **the USB link was never the limit, and neither was the panel.** The panel
acknowledges a telemetry frame in **0.49 ms**. The 1 Hz cadence came from two host-side
mistakes, and at 30 Hz the only remaining ceiling is the Windows timer.

## 1. The blocking read that set the rate

`WindowsSampler::sample` used to call `PipeSensor::next_sample` inline. The vendor sensor
bridge pushes one snapshot every **~1.13 s**, so that read blocked the whole stream.

Measured directly against the bridge (`PipeSensor` in a standalone binary):

| read | time |
|---|---|
| 1 | 145 ms (first, already buffered) |
| 2 | 1.1488 s |
| 3 | 1.1290 s |
| 4 | 1.1285 s |

The daemon's own counters told the same story at a 100 ms request:

```
interval_ms 100
measured_interval_ms 1140
sample_us   1108004     <- the whole second is here
transfer_us      490     <- the panel took half a millisecond
max_transfer_us 1835
failed 0
```

The same measurements identify who owns that pipe: **`SensorBridgeServer.exe`**, installed as
the *Deep Cool Helper Service* (`C:\DeepCool\resources\service\x64\`). It runs independently
of `MystiqueService`, which is why the pipe was still there with the display service stopped.
Stop the helper and the sampler falls back to `sysinfo` with no code change — that is the
path Linux always takes.

This also corrects an earlier belief recorded in `HANDOVER.md`: "the old 1 Hz limit was
DeepCool's sensor bridge". The bridge's 1.1 s push is real, but it was never a rate limit
until we blocked on it. DeepCool's own app also streams at that cadence, which is why the
mistake survived so long.

## 2. The fix: freshness is not rate

`PipeFeed` (`crates/mystique-sensors/src/pipe.rs`) owns the blocking read on its own
thread and keeps the newest snapshot in a mutex. `WindowsSampler::sample` copies it —
microseconds — and falls back to `sysinfo` if the snapshot is older than 3 s.

A 1.1 s snapshot on a 30 Hz stream means the panel animates at 30 Hz and the **numbers
change about once a second**, because that is how often the vendor measures them. Sending
faster does not make the sensors fresher, and the code should not pretend otherwise.

`sysinfo` was measured too, to be sure it was not the culprit: `refresh_cpu_usage` 65–140 µs,
`refresh_memory` 45–55 µs, `components.refresh` 2.7–9.6 ms. Fast enough to be a fallback,
slow enough that it is not the preferred path at 30 Hz.

## 3. Scheduler: work time must not shorten the period

`next_tick = now + interval` adds the frame's own cost to every period. At 1 Hz that was
invisible; at 30 Hz it costs a frame every few seconds. `advance(deadline, now, interval)`
keeps the original phase, catches up by at most one interval, and resets if the work
overruns by more than that.

## 4. Ramp, measured with a client counting the daemon's telemetry events

`re/tools/measure-rate.mjs <seconds> [pipe] [hz]` — counts events, reports gaps and the
daemon's own counters.

| request | achieved | p50 gap | p95 gap | max transfer | failed |
|---|---|---|---|---|---|
| 1000 ms | 0.88 Hz (before the sampler fix) | 1135.75 ms | 1164.91 ms | 1.8 ms | 0 |
| 100 ms | **9.99 Hz** | 94.86 ms | 110.38 ms | 1.9 ms | 0 |
| 50 ms | **20.0 Hz** | 47.15 ms | 63.08 ms | 4.3 ms | 0 |
| 33 ms | **30.26 Hz** | 31.43 ms | 46.69 ms | 3.3 ms | 0 |
| 16 ms | 62.17 Hz, jittery | 15.61 ms | 29.49 ms | 4.4 ms | 0 |
| 8 ms | 89.36 Hz, `measured 12 ms` | 15.03 ms | 16.43 ms | 4.4 ms | 0 |

A live change from a running 1 Hz daemon to 30 Hz, without stopping the stream:

```
{"frames":302,"span_ms":10002,"achieved_hz":30.1,
 "gap_ms":{"min":0.37,"p50":31.3,"p95":47.3,"max":48.38},
 "reported":{"interval_ms":33,"measured_interval_ms":32,
             "sample_us":8,"transfer_us":630,"max_transfer_us":5720,"sent":301,"failed":0}}
```

## 5. Where the ceiling actually is

At 16 ms and below, the `min` gap collapses to 0.4 ms and short gaps appear: the worker
sleeps in `recv_timeout`, and Windows' default timer resolution is **15.625 ms**. A request
below that either overshoots or fires immediately after catching up.

So:

- **30 Hz (33 ms) is the clean ceiling on Windows** with the default timer resolution.
- 60 Hz would need `timeBeginPeriod(1)` or a waitable timer; it is not a device question.
- The bus budget was never relevant: a frame is 48 bytes out and 48 back. At 30 Hz that is
  2.9 KB/s, about **0.19%** of a 12 Mbit/s full-speed link.

## 6. What this does and does not change on screen

- The panel is told nothing about the rate. No command exists for it; it is a host property.
- Numbers still update at the sensor rate. Smoothness, transitions and any panel-side
  animation are what a higher frame rate buys.
- **Untested:** the Recording mode graphs. If the panel advances those graphs per received
  frame, 30 Hz will scroll them 30× faster than the vendor app. Worth one look with the
  graphs enabled before making 30 Hz a default; 1 Hz stays the default until then.

## 7. Interface

- `{"id":N,"cmd":"set_stream_rate","args":{"hz":30}}` → `TelemetryStatus`; `hz` is
  `1..=120`, outside that it answers `ok:false` code `range`.
- `TelemetryStatus` gained `frames_failed`, `measured_interval_ms`, `last_sample_us`,
  `last_transfer_us`, `max_transfer_us`.
- The CLI keeps `--interval-ms`, now `8..=60000` (33 ms = 30 Hz).
- The UI shows the **counted** rate in the status bar (`1.0 HZ`, `30.1 HZ`), counted over a
  one-second window. Without it a working change looks like no change: the panel's numbers
  are sensor-bound, so 1 Hz and 30 Hz render the same digits.
- Every setting control writes when it changes. There is no Send button for display areas,
  media timing, labels or the upload plan; only irreversible actions (upload, erase) keep a
  confirm step.
