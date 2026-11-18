// The live panel preview. It reads the staged display-area selects plus the last
// settings and frame, and draws the layout the panel is believed to show.

import { el, setClass, setHidden, setText } from './dom.js';
import { state } from './state.js';

const history = { freq: [], temp: [] };

function clockText(mhz) {
  const value = Number(mhz);
  return Number.isFinite(value) && value > 0 ? (value / 1000).toFixed(2) : null;
}

function readArea(id, fallback) {
  const node = el(id);
  return node ? Number(node.value) : Number(fallback || 0);
}

function paintCell(index, value, unit, key, show) {
  setHidden('preview-cell-' + index, !show);
  if (!show) return;
  setText('preview-v' + index, value);
  setText('preview-u' + index, unit);
  setText('preview-k' + index, key);
}

function paintAux(aux, frame) {
  const temp = frame ? frame.cpu_temperature_c : null;
  const clock = frame ? clockText(frame.cpu_clock_mhz) : null;

  if (aux === 1) {
    paintCell(1, 'SYS', 'MON', '', true);
    paintCell(2, '', '', '', false);
    paintCell(3, '', '', '', false);
    return;
  }
  if (aux === 2) {
    paintCell(1, temp === null || temp === undefined ? '—' : temp, '°C', 'TEMP', true);
    paintCell(2, clock === null ? '—' : clock, 'GHz', 'CLOCK', true);
    paintCell(3, '', '', '', false);
    return;
  }
  paintCell(1, '—', 'V', '3.3V', true);
  paintCell(2, '—', 'V', '5V', true);
  paintCell(3, '—', 'V', '12V', true);
}

function paintMain(label, value, unit) {
  setText('preview-label', label);
  setText('preview-value', value);
  setText('preview-unit', unit);
}

function paintSensors(cpuTempInPanel) {
  const frame = state.lastFrame;
  const cpu = frame ? frame.cpu_temperature_c : null;
  const gpu = frame ? frame.gpu_temperature_c : null;
  setText('preview-cpu-value', cpu === null || cpu === undefined ? '—' : cpu);
  setText('preview-gpu-value', gpu === null || gpu === undefined ? '—' : gpu);
  // The footer only carries a sensor the panel itself is not drawing. That keeps
  // every live value on screen exactly once.
  setHidden('preview-cpu', cpuTempInPanel);
}

function renderMonitor(settings, frame) {
  const main = readArea('area-main', settings ? settings.main_area : 0);
  const style = readArea('area-style', settings ? settings.style_area : 0);
  const aux = readArea('area-aux', settings ? settings.aux_area : 0);
  const temp = frame ? frame.cpu_temperature_c : null;
  const clock = frame ? clockText(frame.cpu_clock_mhz) : null;

  setHidden('preview-main2', main !== 4);
  setHidden('preview-style', main !== 0);
  setText('preview-style', ['NORMAL', 'RING', 'STOPWATCH'][style] || 'NORMAL');
  setClass('preview-screen', 'ring', main === 0 && style === 1);

  if (main === 1) {
    const now = new Date();
    const hh = String(now.getHours()).padStart(2, '0');
    const mm = String(now.getMinutes()).padStart(2, '0');
    paintMain(now.getHours() < 12 ? 'AM' : 'PM', hh + ':' + mm, '');
  } else if (main === 2) {
    paintMain('PUMP RPM', '—', 'RPM');
  } else if (main === 3) {
    paintMain('CPU FAN', '—', 'RPM');
  } else if (main === 4) {
    paintMain('CPU FAN', '—', 'RPM');
    setText('preview-value2', '—');
    setText('preview-unit2', 'RPM');
  } else if (main === 5) {
    paintMain('CPU TEMPERATURE', temp === null || temp === undefined ? '—' : String(temp), '°C');
  } else if (!frame) {
    paintMain('CPU FREQUENCY', '—', 'GHz');
  } else {
    paintMain('CPU FREQUENCY', clock === null ? '—' : clock, 'GHz');
  }

  paintAux(aux, frame);
  paintSensors(main === 5 || aux === 2);
}

function renderMedia() {
  const box = el('preview-media');
  const img = el('panel-media');
  const has = Boolean(state.mediaPreview);
  if (box) box.classList.toggle('hasimg', has);
  if (img) {
    if (has) img.src = state.mediaPreview;
    else img.removeAttribute('src');
  }
  paintSensors(false);
}

function drawLine(id, values, pad) {
  const line = el(id);
  if (!line) return;
  if (values.length === 0) {
    line.setAttribute('points', '');
    return;
  }
  let min = Math.min(...values);
  let max = Math.max(...values);
  if (max - min < pad) {
    min -= pad / 2;
    max += pad / 2;
  }
  const span = max - min || 1;
  const last = Math.max(values.length - 1, 1);
  const points = values.map((value, index) => {
    const x = (index / last) * 200;
    const y = 66 - ((value - min) / span) * 60;
    return x.toFixed(1) + ',' + y.toFixed(1);
  });
  if (values.length === 1) points.push('200,' + points[0].split(',')[1]);
  line.setAttribute('points', points.join(' '));
}

function renderRecording(settings, frame) {
  const clockOn = !settings || settings.record_cpu_clock !== false;
  const tempOn = !settings || settings.record_cpu_temp !== false;
  const clock = frame ? clockText(frame.cpu_clock_mhz) : null;
  const temp = frame ? frame.cpu_temperature_c : null;
  setHidden('rec-freq-block', !clockOn);
  setHidden('rec-temp-block', !tempOn);
  setHidden('rec-axis', !clockOn && !tempOn);
  setText('rec-freq', clock === null ? '— GHz' : clock + ' GHz');
  setText('rec-temp', temp === null || temp === undefined ? '— °C' : temp + ' °C');
  drawLine('rec-freq-line', history.freq, 0.3);
  drawLine('rec-temp-line', history.temp, 4);
  paintSensors(tempOn);
}

export function pushFrame(frame) {
  if (!frame) return;
  const clock = clockText(frame.cpu_clock_mhz);
  if (clock !== null) history.freq.push(Number(clock));
  if (frame.cpu_temperature_c !== null && frame.cpu_temperature_c !== undefined) {
    history.temp.push(Number(frame.cpu_temperature_c));
  }
  while (history.freq.length > 30) history.freq.shift();
  while (history.temp.length > 30) history.temp.shift();
}

export function renderPreview() {
  const body = el('device-body');
  const panel = el('preview-screen');
  if (!body || !panel) return;

  const mode = body.dataset.mode || 'monitor';
  const settings = state.lastSettings;
  const frame = state.lastFrame;

  panel.dataset.view = mode;
  const rotation = settings ? Number(settings.rotation || 0) : 0;
  panel.dataset.orient = rotation === 1 || rotation === 3 ? 'landscape' : 'portrait';

  const lit = !settings || Boolean(settings.screen_on);
  const brightness = settings ? Number(settings.brightness) : 70;
  panel.classList.toggle('screenoff', !lit || brightness === 0);

  if (mode === 'media') renderMedia();
  else if (mode === 'recording') renderRecording(settings, frame);
  else renderMonitor(settings, frame);
}

export function bindPreviewInputs() {
  for (const id of [
    'area-main',
    'area-style',
    'area-aux',
    'brightness-slider',
    'gyro-toggle',
    'record-freq',
    'record-temp',
  ]) {
    const node = el(id);
    if (node) node.addEventListener('input', renderPreview);
    if (node) node.addEventListener('change', renderPreview);
  }
  renderPreview();
}
