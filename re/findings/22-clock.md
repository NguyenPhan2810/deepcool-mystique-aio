# 22 — `0x0A` is the clock, not a heartbeat

Date: 2026-11-19. Evidence: `re/captures/usbpcap-time-sync.pcap`, captured while the
vendor service and app started, plus the panel's own display.

## The symptom

The panel's `Time` area counted from power-on. At 10:06 with the PC booted at 08:42 it
read `01:23 AM` — exactly the uptime, not the wall clock. The vendor app showed the
correct time on the panel.

## The cause

`0x0A` was documented as a mode switch with a big-endian counter:

```
EA 07 0B 12 0A <counter>
```

The driver replayed that constant prefix forever. It is not a prefix:

```
EA 07 | <month> <day> | <hour> <minute> <second>
```

What looked like a constant `0B 12 0A` was the capture's own date and hour — 18 November,
10:xx — and what looked like a counter was the **seconds** byte. Ten minutes of capture
show it moving 0x0B → 0x0C at the minute boundary with the seconds rolling to 00:

```
12.13s  ea 07 0b 13 0a 0b 2e      2026-11-19 10:11:46
26.65s  ea 07 0b 13 0a 0c 00      2026-11-19 10:12:00
72.22s  ea 07 0b 13 0a 0c 2e      2026-11-19 10:12:46
```

The first packet of the capture, `ea 07 02 02 02 27 21`, is the device's unsynchronised
default (2 February 02:39:33) before the app pushes the real time.

## Why the byte probe missed it

Probing `0x01` payload bytes at 10:30 could not work: time is not part of the telemetry
frame. It is a separate command on the same bulk pipe, one 48-byte packet per second.

## Fix

- The daemon sends `0x0A` with the local date and time once per second, independent of the
  telemetry stream — stopping the stream no longer freezes the clock.
- `Scheduler`: the clock has its own deadline in `next_wait`, so the worker wakes for it
  even with the stream stopped.
- `time_payload` in the protocol crate builds the seven bytes; `TIME_HEADER` is `EA 07`.

## Still open

- The init sequence and the upload preamble still carry the captured 2026-11-18 payload.
  The periodic clock corrects the panel within a second, so the visible effect is at most a
  brief wrong date during startup or an upload. Making those call sites take the current
  time is a mechanical follow-up, not a protocol question.
- Whether the panel keeps time across a USB reset or a power cycle is untested. It is
  pushed every second while the daemon runs, so it does not matter in practice.
