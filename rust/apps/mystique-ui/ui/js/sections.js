// Event wiring for the device page. Every control calls the real Tauri command;
// failures stay inline in the group that caused them.

import { readCommand, runCommand } from './bridge.js';
import { checkedValue, el, setChecked, setDisabled, setHidden, setText } from './dom.js';
import {
  applyBrightness,
  applyDevice,
  applyOutcome,
  applySettings,
  applyStyleRow,
  eraseStoredContent,
  refreshStatus,
  state,
} from './state.js';
import { renderPreview } from './preview.js';
import { refreshTelemetry } from './telemetry.js';
import { bindLabels } from './labels.js';

function deviceSummary(result) {
  if (result.already_open) return 'already open';
  if (!result.device) return result.message || 'open';
  const device = result.device;
  const endpoints = Array.isArray(device.endpoints) ? device.endpoints.length : 0;
  return 'bus ' + device.bus + ' · addr ' + device.address + ' · ' + endpoints + ' EP';
}

function openServiceOutput(output) {
  const details = el('conn-service-details');
  if (details) details.open = true;
  setText('conn-service-output', output ? output : 'no output');
}

function bindConnection() {
  const open = el('btn-open');
  if (open) open.addEventListener('click', async () => {
    const result = await runCommand('open_device', {}, 'conn');
    if (!result) return;
    setText('conn-device-info', deviceSummary(result));
    await refreshStatus();
    await refreshTelemetry();
  });

  const close = el('btn-close');
  if (close) close.addEventListener('click', async () => {
    const device = await runCommand('close_device', {}, 'conn');
    if (!device) return;
    setText('conn-device-info', 'released · stream stopped');
    applyDevice(device);
    renderPreview();
    await refreshStatus();
    await refreshTelemetry();
  });

  const refresh = el('btn-refresh');
  if (refresh) refresh.addEventListener('click', () => {
    void refreshStatus();
    void refreshTelemetry();
  });

  const stop = el('btn-stop-service');
  if (stop) stop.addEventListener('click', async () => {
    const result = await runCommand('stop_service', {}, 'conn');
    if (!result) return;
    applyDevice(result.device);
    openServiceOutput(result.output);
    setText('conn-device-info', 'vendor service stopped');
    renderPreview();
  });

  const start = el('btn-start-service');
  if (start) start.addEventListener('click', async () => {
    const result = await runCommand('start_service', {}, 'conn');
    if (!result) return;
    applyDevice(result.device);
    openServiceOutput(result.output);
    setText('conn-device-info', 'vendor service started');
    renderPreview();
  });
}

async function sendBrightness(value) {
  const outcome = await runCommand('set_brightness', { brightness: value }, 'brightness');
  if (outcome) applyOutcome(outcome);
  else applySettings(state.lastSettings);
  renderPreview();
}

function bindBrightness() {
  const slider = el('brightness-slider');
  if (slider) {
    slider.addEventListener('input', () => {
      applyBrightness(slider.value);
      renderPreview();
    });
    slider.addEventListener('change', () => { void sendBrightness(Number(slider.value)); });
  }

  const toggle = el('screen-toggle');
  if (toggle) toggle.addEventListener('change', async () => {
    const lit = toggle.checked;
    const outcome = await runCommand('set_screen_power', { lit }, 'brightness');
    if (outcome) applyOutcome(outcome);
    else {
      applySettings(state.lastSettings);
      setChecked('screen-toggle', state.lastSettings ? state.lastSettings.screen_on : true);
    }
    renderPreview();
  });

  for (const value of [0, 25, 50, 75, 100]) {
    const button = el('brightness-preset-' + value);
    if (!button) continue;
    button.addEventListener('click', () => {
      applyBrightness(value);
      renderPreview();
      void sendBrightness(value);
    });
  }
}

function bindRotation() {
  for (const raw of [0, 1, 2, 3]) {
    const button = el('rotation-btn-' + raw);
    if (!button) continue;
    button.addEventListener('click', async () => {
      const outcome = await runCommand('set_rotation', { rotation: raw }, 'rotation');
      if (!outcome) return;
      applyOutcome(outcome);
      renderPreview();
    });
  }
}

function bindGyro() {
  const gyro = el('gyro-toggle');
  if (!gyro) return;
  gyro.addEventListener('change', async () => {
    const outcome = await runCommand('set_gyro', { enabled: gyro.checked }, 'rotation');
    if (outcome) applyOutcome(outcome);
    else applySettings(state.lastSettings);
    renderPreview();
  });
}

function bindIdleScreen() {
  const toggle = el('idle-screen');
  if (!toggle) return;
  toggle.addEventListener('change', async () => {
    const outcome = await runCommand('set_idle_screen', { enabled: toggle.checked }, 'idle');
    if (outcome) applyOutcome(outcome);
    else applySettings(state.lastSettings);
    renderPreview();
  });
}

// The three lighting choices are one interlocked setting, exactly like the vendor app:
// turning one on turns the others off. The radio group makes that impossible to violate.
function bindRgbMode() {
  for (const input of document.querySelectorAll('input[name="rgb-mode"]')) {
    input.addEventListener('change', async () => {
      if (!input.checked) return;
      const outcome = await runCommand('set_rgb_mode', { mode: input.value }, 'rgb');
      if (outcome) applyOutcome(outcome);
      else applySettings(state.lastSettings);
    });
  }
}

function bindFanIo() {
  const cpu = el('io-cpu');
  const pump = el('io-pump');
  if (!cpu || !pump) return;

  // Host routing, not a device command: each dial is told one number. Changing either
  // selector pushes one frame so both dials redraw with the new source.
  const send = async () => {
    const outcome = await runCommand(
      'set_fan_io',
      { cpu: cpu.value, pump: pump.value },
      'fanio',
    );
    if (outcome) applyOutcome(outcome);
    else applySettings(state.lastSettings);
    renderPreview();
  };

  cpu.addEventListener('change', () => { void send(); });
  pump.addEventListener('change', () => { void send(); });
}

function bindRecording() {
  const clock = el('record-freq');
  const temp = el('record-temp');
  if (!clock || !temp) return;
  const send = async () => {
    const outcome = await runCommand(
      'set_recording',
      { cpu_clock: clock.checked, cpu_temp: temp.checked },
      'recording',
    );
    if (outcome) applyOutcome(outcome);
    else applySettings(state.lastSettings);
    renderPreview();
  };
  clock.addEventListener('change', () => { void send(); });
  temp.addEventListener('change', () => { void send(); });
}

function bindAreas() {
  const main = el('area-main');
  const style = el('area-style');
  const aux = el('area-aux');
  if (!main || !style || !aux) return;

  // No Send button: a select is a committed choice, so it goes to the panel. The only
  // reason to hold it back would be a multi-field form, and these three fields are one
  // layout whose intermediate states are all valid.
  const sendAreas = async () => {
    const outcome = await runCommand(
      'set_areas',
      { main: Number(main.value), style: Number(style.value), aux: Number(aux.value) },
      'areas',
    );
    if (outcome) applyOutcome(outcome);
    else applySettings(state.lastSettings);
    renderPreview();
  };

  main.addEventListener('change', () => {
    applyStyleRow(main.value);
    renderPreview();
    void sendAreas();
  });
  style.addEventListener('change', () => {
    renderPreview();
    void sendAreas();
  });
  aux.addEventListener('change', () => {
    renderPreview();
    void sendAreas();
  });
}

function bindMedia() {
  // Same rule: the radios are the setting. Changing one writes it.
  const send = async () => {
    const time = checkedValue('media-time');
    const animation = checkedValue('media-animation');
    if (time === null || animation === null) return;
    const outcome = await runCommand(
      'set_media_timing',
      { time: Number(time), animation: Number(animation) },
      'media',
    );
    if (outcome) applyOutcome(outcome);
    else applySettings(state.lastSettings);
  };

  for (const input of document.querySelectorAll('input[name="media-time"], input[name="media-animation"]')) {
    input.addEventListener('change', () => { void send(); });
  }
}

async function sendErase() {
  const confirm = el('dashboard-confirm');
  const send = el('btn-dashboard-erase');
  if (!confirm || !send || !confirm.checked) return;

  setDisabled('btn-return-dashboard', true);
  setDisabled('btn-dashboard-erase', true);
  setText('dashboard-state', 'Sending 0x14 · no read-back');
  const outcome = await eraseStoredContent('dashboard');
  setDisabled('btn-return-dashboard', false);
  if (!outcome) {
    setDisabled('btn-dashboard-erase', !confirm.checked);
    setText('dashboard-state', 'Erase failed · see log');
    return;
  }

  applyOutcome(outcome);
  setText('dashboard-state', 'Erased · panel should show machine info');
  setText('dashboard-report', outcome.packets.length + ' PKT · 0x14');
  confirm.checked = false;
  setDisabled('btn-dashboard-erase', true);
  setHidden('dashboard-confirm-box', true);
}

function bindDashboard() {
  const arm = el('btn-return-dashboard');
  const box = el('dashboard-confirm-box');
  const confirm = el('dashboard-confirm');
  const send = el('btn-dashboard-erase');
  if (!arm || !box || !confirm || !send) return;

  arm.addEventListener('click', () => {
    setHidden('dashboard-confirm-box', false);
    setText('dashboard-state', 'Confirm: deletes the stored image or GIF');
    confirm.focus();
  });

  confirm.addEventListener('change', () => {
    setDisabled('btn-dashboard-erase', !confirm.checked);
  });

  send.addEventListener('click', () => { void sendErase(); });
}

function bindErase() {
  const erase = el('btn-erase-images');
  if (!erase) return;
  erase.addEventListener('click', async () => {
    setDisabled('btn-erase-images', true);
    setText('erase-message', 'Sending 0x14 · no read-back');
    const outcome = await eraseStoredContent('erase');
    setDisabled('btn-erase-images', false);
    if (!outcome) {
      setText('erase-message', 'Erase failed · see log');
      return;
    }
    applyOutcome(outcome);
    setText('erase-message', outcome.message || '0x14 sent');
  });
}

// Startup toggles live in the session sheet. The OS entry is the source of truth: the
// app reads it back after every change, so removing it by hand is reflected here.
async function bindStartup() {
  const enabled = el('startup-enabled');
  const minimized = el('startup-minimized');
  if (!enabled || !minimized) return;

  const paint = (state) => {
    if (!state) return;
    enabled.checked = Boolean(state.enabled);
    minimized.checked = Boolean(state.minimized);
    minimized.disabled = !state.enabled;
  };

  paint(await readCommand('startup_get'));

  const send = async () => {
    const state = await runCommand(
      'startup_set',
      { enabled: enabled.checked, minimized: minimized.checked },
      'startup',
    );
    paint(state);
  };

  enabled.addEventListener('change', () => {
    minimized.disabled = !enabled.checked;
    void send();
  });
  minimized.addEventListener('change', () => { void send(); });
}

export function bindSections() {
  bindConnection();
  bindBrightness();
  bindRotation();
  bindGyro();
  bindIdleScreen();
  bindRgbMode();
  bindStartup();
  bindFanIo();
  bindRecording();
  bindAreas();
  bindMedia();
  bindLabels();
  bindDashboard();
  bindErase();
}
