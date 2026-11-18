// The daemon link, the session strip and the event bridge. The daemon is the only
// process that touches USB; this window renders what it broadcasts.

import { listen, readCommand, runCommand } from './bridge.js';
import { el, setDisabled, setDot, setHidden, setText, setTitle } from './dom.js';
import { setLogLive } from './log.js';
import { applyDevice, applyStatus } from './state.js';
import { applyFrame, applyTelemetry } from './telemetry.js';
import { renderPreview } from './preview.js';

const CONNECTION = 'daemon-connection';
const STATE = 'daemon-state';
const STATUS = 'daemon-status';
const TELEMETRY = 'daemon-telemetry';

let linkConnected = false;

function phaseWord(payload) {
  if (payload.phase === 'connected') return 'Running';
  if (payload.phase === 'connecting') return 'Contacting';
  return 'Not running';
}

export function applyConnection(payload) {
  if (!payload) return;
  const health = payload.health || {};
  const code = health.code || 'connecting';
  const connected = payload.phase === 'connected';
  linkConnected = connected;

  setText('daemon-status', phaseWord(payload));
  setText('daemon-endpoint', payload.endpoint || '—');
  setText('daemon-detail', health.detail || health.headline || 'no daemon state yet');
  setTitle('daemon-detail', health.detail || health.headline || '');
  setText(
    'daemon-child',
    payload.spawned_by_ui && connected ? '(started by this window)' : connected ? '' : '—',
  );
  setDot('daemon-dot', connected ? 'on' : '');

  setHidden('daemon-conflict', !health.conflict);
  if (health.conflict) {
    setText('daemon-conflict-text', (health.headline || '') + ' · ' + (health.detail || ''));
  }
  setDisabled('btn-daemon-retry', payload.phase === 'connecting');
  setDisabled('btn-daemon-stop', !connected);

  document.documentElement.dataset.daemon = code;
  setLogLive(connected);
  if (!connected) {
    setText('stream-word', 'Idle');
    setDot('stream-dot', '');
  }
}

function applyState(device) {
  if (!device) return;
  applyDevice(device);
  if (device.telemetry) applyTelemetry(device.telemetry);
  renderPreview();
}

function applyLiveStatus(status) {
  if (!status) return;
  applyStatus(status);
  if (status.telemetry) applyTelemetry(status.telemetry);
  renderPreview();
}

// The daemon does not replay events, so a window that opens late asks for one
// snapshot in the daemon's own order: device_state first, then status.
export async function loadSnapshot() {
  const device = await readCommand('device_state');
  if (device) applyState(device);
  const status = await readCommand('status');
  if (status) applyLiveStatus(status);
}

function bindRetry() {
  const retry = el('btn-daemon-retry');
  if (!retry) return;
  retry.addEventListener('click', async () => {
    setDisabled('btn-daemon-retry', true);
    const info = await readCommand('daemon_retry');
    if (info) applyConnection(info);
    await loadSnapshot();
    setDisabled('btn-daemon-retry', false);
  });
}

function bindStop() {
  const stop = el('btn-daemon-stop');
  if (!stop) return;
  stop.addEventListener('click', async () => {
    setDisabled('btn-daemon-stop', true);
    const ack = await runCommand('daemon_stop', {}, 'conn');
    if (!ack) {
      setDisabled('btn-daemon-stop', !linkConnected);
      return;
    }
    setText('daemon-detail', ack.message || 'daemon accepted the stop request');
    setText('conn-device-info', 'daemon stopped · panel released');
    setText('conn-device-open', 'Closed');
    setDot('handle-dot', '');
    setText('stream-word', 'Idle');
    setDot('stream-dot', '');
  });
}

export async function bindDaemon() {
  await Promise.all([
    listen(CONNECTION, applyConnection),
    listen(STATE, applyState),
    listen(STATUS, applyLiveStatus),
    listen(TELEMETRY, applyFrame),
  ]);

  bindRetry();
  bindStop();

  const info = await readCommand('daemon_info');
  if (info) applyConnection(info);
  await loadSnapshot();
}
