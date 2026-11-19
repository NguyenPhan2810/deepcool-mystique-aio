// Live stream counters. The daemon owns the loop and the timing numbers; this module
// paints the counted status-bar rate and the session-sheet counters.

import { readCommand, runCommand } from './bridge.js';
import {
  el,
  formatNumber,
  setClass,
  setDisabled,
  setDot,
  setHidden,
  setText,
} from './dom.js';
import { state } from './state.js';
import { pushFrame, renderPreview } from './preview.js';

let baseline = 0;
let seen = 0;

function paintFrame(frame) {
  if (!frame) return;
  state.lastFrame = frame;
  pushFrame(frame);
  renderPreview();
}

const MISSING = {
  cpu_temperature_c: 'CPU°C',
  gpu_temperature_c: 'GPU°C',
  cpu_usage_percent: 'CPU%',
  memory_load_percent: 'MEM%',
  cpu_clock_mhz: 'CLK',
  pump_rpm: 'PUMP',
  cpu_fan_rpm: 'FAN',
};

function missingText(missing) {
  if (!Array.isArray(missing) || missing.length === 0) return 'MISSING —';
  return 'MISSING ' + missing.map((field) => MISSING[field] || field).join(' ');
}

function paintError(status) {
  const note = el('stream-error-note');
  if (!note) return;
  if (status && status.last_error) {
    note.textContent = status.last_error;
    note.title = status.last_error;
    note.hidden = false;
  } else {
    note.textContent = '';
    note.title = '';
    note.hidden = true;
  }
}

// The status bar shows the rate the daemon is actually achieving, counted over a
// one-second window. The panel's numbers change once per second whatever the frame rate,
// so a counted rate is the only honest progress signal. Counting frames beats averaging
// gaps — gap averages are biased high by bursty scheduling, a count is not.
let windowAt = 0;
let windowSeen = 0;

function paintStream(running) {
  setClass('stream-badge', 'paused', !running);
  setDot('stream-dot', running ? 'on' : '');
  if (!running) {
    windowAt = 0;
    windowSeen = 0;
    setText('stream-word', 'IDLE');
  } else if (windowAt === 0) {
    setText('stream-word', '—');
  }
}

function paintCounters(status) {
  setText('stream-frames', formatNumber(status.frames_sent));
  setText('stream-failed', formatNumber(status.frames_failed || 0));

  const sample = Number(status.last_sample_us) || 0;
  const transfer = Number(status.last_transfer_us) || 0;
  const max = Number(status.max_transfer_us) || 0;
  setText('stream-sample', (sample / 1000).toFixed(1));
  setText('stream-transfer', (transfer / 1000).toFixed(1));
  setText('stream-max', (max / 1000).toFixed(1));

  const requested = Number(status.interval_ms) > 0 ? 1000 / Number(status.interval_ms) : 0;
  const measured =
    Number(status.measured_interval_ms) > 0 ? 1000 / Number(status.measured_interval_ms) : 0;
  if (requested > 0 && measured > 0) {
    const delta = (measured - requested) / requested;
    if (Math.abs(delta) > 0.05) {
      const sign = delta > 0 ? '+' : '';
      setText('stream-achieved', measured.toFixed(1) + ' HZ · ' + sign + Math.round(delta * 100) + '%');
      setHidden('stream-achieved-row', false);
      return;
    }
  }
  setHidden('stream-achieved-row', true);
}

export function applyTelemetry(status) {
  if (!status) return null;
  const running = Boolean(status.running);

  baseline = Number(status.frames_sent) || 0;
  seen = 0;
  windowAt = 0;
  windowSeen = 0;

  paintStream(running);
  paintCounters(status);
  setText('stream-source', status.source || (running ? 'waiting' : 'not started'));
  setText('stream-missing', missingText(status.missing));
  paintError(status);

  setDisabled('btn-stream-start', running || !status.device_open);
  setDisabled('btn-stream-stop', !running);

  if (status.last_frame) paintFrame(status.last_frame);
  return status;
}

export function applyFrame(frame) {
  if (!frame) return;
  seen += 1;
  const now = performance.now();
  if (windowAt === 0) {
    windowAt = now;
    windowSeen = seen;
  } else if (now - windowAt >= 1000) {
    const hz = ((seen - windowSeen) * 1000) / (now - windowAt);
    setText('stream-word', hz.toFixed(1) + ' HZ');
    windowAt = now;
    windowSeen = seen;
  }
  paintFrame(frame);
  setText('stream-frames', formatNumber(baseline + seen));
  setText('stream-source', frame.source || 'daemon');
  setText('stream-missing', missingText(frame.missing));
  paintStream(true);
  paintError(null);
}

export async function refreshTelemetry() {
  const status = await readCommand('telemetry_status');
  return status ? applyTelemetry(status) : null;
}

function bindStart() {
  const start = el('btn-stream-start');
  if (!start) return;
  start.addEventListener('click', async () => {
    setDisabled('btn-stream-start', true);
    const status = await runCommand('start_telemetry', {}, 'stream');
    if (status) applyTelemetry(status);
    else await refreshTelemetry();
  });
}

function bindStop() {
  const stop = el('btn-stream-stop');
  if (!stop) return;
  stop.addEventListener('click', async () => {
    setDisabled('btn-stream-stop', true);
    const status = await runCommand('stop_telemetry', {}, 'stream');
    if (status) applyTelemetry(status);
    else await refreshTelemetry();
  });
}

// The timing counters are a snapshot, not a poll: opening the sheet asks for one status
// update, and later state events (rate changes, errors) refresh them. Polling here would
// spawn sc.exe and enumerate USB once a second, which is not acceptable while the stream
// is being measured.
function bindSessionCounters() {
  const details = el('session-details');
  if (!details) return;
  details.addEventListener('toggle', () => {
    if (details.open) void refreshTelemetry();
  });
}

export function bindTelemetry() {
  bindStart();
  bindStop();
  bindSessionCounters();
}
