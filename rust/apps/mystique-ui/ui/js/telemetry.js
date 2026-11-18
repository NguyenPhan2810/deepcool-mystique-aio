// Live stream counters. The daemon owns the 1 Hz loop and the frame count; this
// module paints the status bar and the preview.

import { readCommand, runCommand } from './bridge.js';
import { el, formatNumber, setClass, setDisabled, setDot, setText } from './dom.js';
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

function paintStream(running) {
  setText('stream-word', running ? 'Live' : 'Idle');
  setClass('stream-badge', 'paused', !running);
  setDot('stream-dot', running ? 'on' : '');
}

export function applyTelemetry(status) {
  if (!status) return null;
  const running = Boolean(status.running);

  baseline = Number(status.frames_sent) || 0;
  seen = 0;

  paintStream(running);
  setText('stream-frames', formatNumber(baseline));
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

export function bindTelemetry() {
  bindStart();
  bindStop();
}
