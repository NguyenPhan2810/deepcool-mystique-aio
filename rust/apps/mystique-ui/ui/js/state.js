// Shared UI state and the DTO-to-control mapping. Every number here comes from
// SettingsState, StatusDto or DeviceState; the panel preview is a belief built
// from these values, never a device read.

import { runCommand } from './bridge.js';
import {
  el,
  setChecked,
  setClass,
  setDisabled,
  setDot,
  setHidden,
  setPressed,
  setSelect,
  setText,
} from './dom.js';
import { logPackets } from './log.js';

export const state = {
  lastSettings: null,
  lastFrame: null,
  mediaPreview: null,
  mediaKind: null,
  selectedPath: null,
  plan: null,
  planPath: null,
  planFit: null,
  uploading: false,
};

const encoder = new TextEncoder();

export function byteLength(text) {
  return encoder.encode(String(text)).length;
}

export function updateLabelCounters() {
  for (const side of ['cpu', 'gpu']) {
    const input = el('label-' + side);
    const counter = el(side + '-counter');
    const error = el(side + '-label-error');
    if (!input || !counter) continue;
    const bytes = byteLength(input.value);
    setText(side + '-count', bytes);
    counter.classList.toggle('over', bytes > 39);
    if (error) {
      const over = bytes - 39;
      error.hidden = over <= 0;
      error.textContent = over > 0
        ? over + (over === 1 ? ' byte' : ' bytes') + ' over 39 B'
        : '';
      error.title = error.textContent;
    }
  }
}

function optionText(id, value) {
  const select = el(id);
  const index = Number(value);
  if (select && Number.isInteger(index) && select.options[index]) {
    return select.options[index].textContent;
  }
  return String(value);
}

export function applyBrightness(value) {
  const brightness = Math.max(0, Math.min(100, Math.round(Number(value) || 0)));
  const slider = el('brightness-slider');
  if (slider && document.activeElement !== slider) slider.value = String(brightness);
  if (slider) slider.setAttribute('aria-valuetext', brightness + ' percent');

  setText('brightness-value', brightness);
  const fill = el('brightness-fill');
  if (fill) fill.style.width = 'calc(' + brightness + '% - ' + (brightness * 0.14) + 'px)';

  for (const preset of [0, 25, 50, 75, 100]) {
    setPressed('brightness-preset-' + preset, preset === brightness);
  }
}

export function applySettings(settings) {
  if (!settings) return;
  state.lastSettings = settings;

  applyBrightness(settings.brightness);

  for (const raw of [0, 1, 2, 3]) {
    setPressed('rotation-btn-' + raw, Number(settings.rotation) === raw);
  }
  setHidden('rotation-warn', !settings.gyro);

  const lit = Boolean(settings.screen_on);
  setChecked('screen-toggle', lit);
  setText('screen-state',
    lit ? '' : 'Screen off · restores ' + settings.screen_restore_brightness + '%');
  setHidden('screen-line', lit);

  setChecked('gyro-toggle', Boolean(settings.gyro));
  setChecked('idle-screen', settings.idle_screen !== false);
  setChecked('rgb-sync', Boolean(settings.rgb_sync));
  setChecked('record-freq', settings.record_cpu_clock !== false);
  setChecked('record-temp', settings.record_cpu_temp !== false);

  setSelect('area-main', settings.main_area);
  setSelect('area-style', settings.style_area);
  setSelect('area-aux', settings.aux_area);
  applyStyleRow(settings.main_area);

  setSelect('io-cpu', settings.fan_io_cpu || 'cpu');
  setSelect('io-pump', settings.fan_io_pump || 'pump');

  setChecked('media-time-' + settings.media_time, true);
  setChecked('media-animation-' + settings.media_animation, true);

  const cpu = el('label-cpu');
  const gpu = el('label-gpu');
  if (cpu && document.activeElement !== cpu) cpu.value = settings.cpu_label;
  if (gpu && document.activeElement !== gpu) gpu.value = settings.gpu_label;
  updateLabelCounters();
}

// Style Type is conditional on CPU Frequency, exactly as the vendor removes it
// for the other five main layouts.
export function applyStyleRow(mainArea) {
  setHidden('area-style-row', Number(mainArea) !== 0);
}

export async function eraseStoredContent(section) {
  return runCommand('erase_images', {}, section);
}

export function applyDevice(device) {
  if (!device) return;

  const present = Boolean(device.device_present);
  const open = Boolean(device.device_open);
  const supported = Boolean(device.service_supported);
  const running = Boolean(device.service_running);

  setText('conn-device-present', present ? 'PRESENT' : 'ABSENT');
  setText('conn-device-open', open ? 'Open' : 'Closed');
  setText('conn-service-running', supported ? (running ? 'Running' : 'Stopped') : 'N/A');

  setDot('handle-dot', open ? 'on' : '');
  setDot('service-dot', running ? 'on' : '');
  setHidden('conn-service-warning', !(supported && running) || open);
  setText('conn-service-output', device.service_output ? device.service_output : 'no output');
  setText('conn-last-error', device.last_error ? device.last_error : 'none');

  setDisabled('btn-open', open);
  setDisabled('btn-close', !open);
  setDisabled('btn-stop-service', !(supported && running));
  setDisabled('btn-start-service', !(supported && !running));
  setClass('btn-open', 'dark', !open && supported && !running);
  setClass('btn-close', 'dark', open);
  setClass('btn-stop-service', 'dark', supported && running);

  applySettings(device.settings);
}

function shortSource(source) {
  if (source.includes('0x10')) return '0x10';
  if (source.includes('session')) return 'session';
  return 'unknown';
}

export function applyStatus(status) {
  if (!status) return;
  applyDevice(status);

  setText('conn-rotation-source', shortSource(String(status.rotation_source || 'unknown')));

  if (!status.mode) {
    setText('conn-mode', '0x10 · not read');
    return;
  }
  const raw = Number.isInteger(status.mode_raw)
    ? '0x' + Number(status.mode_raw).toString(16).padStart(2, '0').toUpperCase()
    : '0x??';
  const result = status.result ? ' · ' + status.result : '';
  setText('conn-mode', '0x10 · ' + status.mode + ' · ' + raw + result);
}

export function applyOutcome(outcome) {
  if (!outcome) return null;
  applySettings(outcome.settings);
  logPackets(outcome.packets);
  return outcome;
}

export async function refreshStatus() {
  const status = await runCommand('status', {}, 'conn');
  if (status) applyStatus(status);
  return status;
}
