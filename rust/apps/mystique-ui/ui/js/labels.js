// Labels: the text input is the value. The select beside it is a shortcut that lists what
// the host detected, fills the input and commits at once; it never holds a value itself.

import { readCommand, runCommand } from './bridge.js';
import { el, setHidden } from './dom.js';
import { applyOutcome, updateLabelCounters } from './state.js';

const CUSTOM = '__custom__';
const SIDES = ['cpu', 'gpu'];

function inputFor(side) {
  return el('label-' + side);
}

async function commit() {
  const cpu = inputFor('cpu');
  const gpu = inputFor('gpu');
  if (!cpu || !gpu) return;
  const outcome = await runCommand('set_labels', { cpu: cpu.value, gpu: gpu.value }, 'labels');
  if (outcome) applyOutcome(outcome);
}

function isPicker(node) {
  return Boolean(node && node.dataset && node.dataset.pick === 'label');
}

function wireTyping() {
  let timer = null;
  const cancel = () => {
    if (timer) clearTimeout(timer);
    timer = null;
  };

  for (const side of SIDES) {
    const input = inputFor(side);
    if (!input) continue;

    input.addEventListener('input', () => {
      updateLabelCounters();
      cancel();
      timer = setTimeout(() => {
        timer = null;
        void commit();
      }, 500);
    });

    // Moving into the picker is not leaving the field: the pick commits the value, so a
    // blur commit here would send the old text first.
    input.addEventListener('blur', (event) => {
      if (isPicker(event.relatedTarget)) {
        cancel();
        return;
      }
      cancel();
      void commit();
    });

    input.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter') return;
      event.preventDefault();
      cancel();
      void commit();
    });
  }
}

// The disabled first option keeps the control reading as a menu, not as a stored value.
// Every detected name is listed in order; `Custom` is always last.
function fillOptions(side, names) {
  const select = el('label-' + side + '-pick');
  const wrap = el(side + '-pick-wrap');
  if (!select || !wrap) return;
  if (!Array.isArray(names) || names.length === 0) return;

  select.length = 1;
  for (const name of names) {
    const option = document.createElement('option');
    option.value = name;
    option.textContent = name;
    select.appendChild(option);
  }
  const custom = document.createElement('option');
  custom.value = CUSTOM;
  custom.textContent = 'Custom';
  select.appendChild(custom);

  select.selectedIndex = 0;
  setHidden(side + '-pick-wrap', false);
}

function wirePicker(side) {
  const select = el('label-' + side + '-pick');
  const input = inputFor(side);
  if (!select || !input) return;

  select.addEventListener('change', () => {
    const value = select.value;
    select.selectedIndex = 0;
    if (!value) return;
    if (value === CUSTOM) {
      input.focus();
      return;
    }
    input.value = value;
    updateLabelCounters();
    void commit();
  });
}

// The window starts before the daemon is connected, and a spawn takes a few seconds, so
// the first probe can come back empty. Retry for about ten seconds instead of giving up:
// a picker that only appears after a manual reload is a bug, not a state.
async function loadDetected(attempts = 9) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const detected = await readCommand('detected_labels');
    const hasCpu = Boolean(detected && detected.cpu && detected.cpu.length);
    const hasGpu = Boolean(detected && detected.gpu && detected.gpu.length);
    if (hasCpu || hasGpu) {
      fillOptions('cpu', detected.cpu);
      fillOptions('gpu', detected.gpu);
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 1200));
  }
}

export function bindLabels() {
  wireTyping();
  wirePicker('cpu');
  wirePicker('gpu');
  void loadDetected();
}
