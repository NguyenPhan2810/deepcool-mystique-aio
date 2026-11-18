// Measures the daemon's achieved telemetry rate on the real link.
//
// The daemon broadcasts one `telemetry` event per frame it wrote *and* had acknowledged,
// so the spacing of these events is the end-to-end period: schedule, USB round trip and
// sampler. A period that grows past the requested interval means the panel is not keeping
// up, not that the host is slow.
//
// Usage: node measure-rate.mjs [seconds] [pipe]
// Prints one JSON object: count, hz, gaps in ms, and any stream errors seen.

import net from "node:net";

const seconds = Number(process.argv[2] ?? 10);
const pipe = process.argv[3] ?? String.raw`\\.\pipe\mystique-control-v1`;
const hz = process.argv[4] ? Number(process.argv[4]) : null;

const socket = net.connect(pipe);
const stamps = [];
let errors = [];
let started = 0;
let buffer = "";

socket.setEncoding("utf8");
socket.on("connect", () => {
  if (hz) socket.write(JSON.stringify({ id: 2, cmd: "set_stream_rate", args: { hz } }) + "\n");
  socket.write(JSON.stringify({ id: 1, cmd: "start_stream", args: {} }) + "\n");
  started = performance.now();
});
socket.on("error", (error) => {
  console.log(JSON.stringify({ error: String(error) }));
  process.exit(1);
});
socket.on("data", (chunk) => {
  buffer += chunk;
  let cut;
  while ((cut = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, cut);
    buffer = buffer.slice(cut + 1);
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (message.event === "telemetry") {
      stamps.push(performance.now());
    } else if (message.event === "state") {
      const status = message.state?.telemetry;
      if (status?.last_error) errors.push(status.last_error);
    } else if (message.id === 99 && message.ok) {
      report(message.data?.telemetry ?? null);
    }
  }
});

const finish = () => {
  socket.write(JSON.stringify({ id: 99, cmd: "device_state", args: {} }) + "\n");
};

let reported = false;
const report = (status) => {
  if (reported) return;
  reported = true;
  const gaps = [];
  for (let i = 1; i < stamps.length; i += 1) gaps.push(stamps[i] - stamps[i - 1]);
  gaps.sort((a, b) => a - b);
  const at = (q) => (gaps.length ? Number(gaps[Math.min(gaps.length - 1, Math.floor(gaps.length * q))].toFixed(2)) : null);
  const span = stamps.length > 1 ? stamps[stamps.length - 1] - stamps[0] : 0;
  console.log(
    JSON.stringify({
      frames: stamps.length,
      span_ms: Number(span.toFixed(0)),
      achieved_hz: span > 0 ? Number(((stamps.length - 1) / (span / 1000)).toFixed(2)) : null,
      gap_ms: { min: at(0), p50: at(0.5), p95: at(0.95), max: at(1) },
      dropped_frames: gaps.length ? gaps.filter((g) => g > 2 * at(0.5)).length : 0,
      reported: status
        ? {
            interval_ms: status.interval_ms,
            measured_interval_ms: status.measured_interval_ms,
            sample_us: status.last_sample_us,
            transfer_us: status.last_transfer_us,
            max_transfer_us: status.max_transfer_us,
            sent: status.frames_sent,
            failed: status.frames_failed,
          }
        : null,
      errors: [...new Set(errors)].slice(0, 3),
    }),
  );
  socket.destroy();
  process.exit(0);
};

setTimeout(finish, seconds * 1000);
setTimeout(() => report(null), seconds * 1000 + 4000);
