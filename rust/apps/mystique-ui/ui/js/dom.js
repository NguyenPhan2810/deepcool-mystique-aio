// Minimal DOM helpers. Behaviour is keyed on ids, never on classes or nesting.

export function el(id) {
  return document.getElementById(id);
}

export function setText(id, value) {
  const node = el(id);
  if (node) node.textContent = value === null || value === undefined ? '' : String(value);
}

export function setTitle(id, value) {
  const node = el(id);
  if (node) node.title = value === null || value === undefined ? '' : String(value);
}

export function setHidden(id, hidden) {
  const node = el(id);
  if (node) node.hidden = Boolean(hidden);
}

export function setDisabled(id, disabled) {
  const node = el(id);
  if (node) node.disabled = Boolean(disabled);
}

export function setPressed(id, pressed) {
  const node = el(id);
  if (node) node.setAttribute('aria-pressed', pressed ? 'true' : 'false');
}

export function setChecked(id, checked) {
  const node = el(id);
  if (node) node.checked = Boolean(checked);
}

export function setSelect(id, value) {
  const node = el(id);
  if (node) node.value = String(value);
}

export function setClass(id, name, on) {
  const node = el(id);
  if (node) node.classList.toggle(name, Boolean(on));
}

// Every session lamp is the same primitive: `lamp` plus `on` or nothing.
export function setDot(id, mark) {
  const node = el(id);
  if (node) node.className = 'lamp' + (mark ? ' on' : '');
}

export function formatNumber(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number.toLocaleString('en-US') : '—';
}

export function checkedValue(name) {
  const node = document.querySelector('input[name="' + name + '"]:checked');
  return node ? node.value : null;
}
