// Inline errors, raw. The backend's code, message and hint are kept as written;
// the visual line is clamped, the title carries the whole string.

import { el } from './dom.js';

export function clearError(section) {
  const node = el(section + '-error');
  if (!node) return;
  node.textContent = '';
  node.title = '';
  node.hidden = true;
}

export function describeError(error) {
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object') {
    const code = error.code ? error.code + ' · ' : '';
    const message = error.message || 'the command failed';
    const hint = error.hint ? ' · ' + error.hint : '';
    return code + message + hint;
  }
  return String(error);
}

export function showError(section, error) {
  const node = el(section + '-error');
  if (!node) return;
  const text = describeError(error);
  node.textContent = text;
  node.title = text;
  node.hidden = false;
}
