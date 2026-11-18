// The packet log drawer. Rows stream while the drawer is closed; opening it
// shows the same list, and a row expands to the exact bytes that went out.

import { el, formatNumber, setHidden, setText } from './dom.js';

let total = 0;
let follow = true;

function stamp() {
  const now = new Date();
  const pad = (value) => String(value).padStart(2, '0');
  return pad(now.getHours()) + ':' + pad(now.getMinutes()) + ':' + pad(now.getSeconds());
}

function commandText(packet) {
  if (packet.code === null || packet.code === undefined) return '—';
  const code = Number(packet.code);
  if (!Number.isFinite(code)) return '—';
  return '0x' + code.toString(16).toUpperCase().padStart(2, '0');
}

function resultText(result) {
  const text = String(result || '');
  if (text === 'success') return 'OK';
  if (text.startsWith('error')) return 'ERR';
  if (text.startsWith('sent')) return 'SENT';
  return text ? text.slice(0, 4).toUpperCase() : '—';
}

function isError(packet) {
  return String(packet.result || '').startsWith('error');
}

function span(className, value) {
  const node = document.createElement('span');
  node.className = className;
  node.textContent = value;
  return node;
}

function buildRow(packet) {
  const item = document.createElement('li');
  item.className = 'log-item';

  const details = document.createElement('details');
  const summary = document.createElement('summary');
  summary.className = 'logrow' + (isError(packet) ? ' err' : '');

  const payload = packet.payload_hex || '';
  summary.append(
    span('lt', stamp()),
    span('lc', commandText(packet)),
    span('lp' + (payload ? '' : ' none'), payload || '—'),
    span('la', '→'),
    span('lr', resultText(packet.result)),
  );
  summary.title = packet.label || 'packet';

  const body = document.createElement('div');
  body.className = 'logdetail';
  const label = document.createElement('div');
  label.className = 'lbl';
  label.textContent = (packet.label || 'packet') + (packet.pipe ? ' · ' + packet.pipe : '');
  body.append(label);

  const sent = document.createElement('pre');
  sent.textContent = 'sent  ' + (packet.packet_hex || '(no bytes recorded)');
  body.append(sent);

  if (packet.reply_hex) {
    const reply = document.createElement('pre');
    reply.textContent = 'reply ' + packet.reply_hex;
    body.append(reply);
  }

  details.append(summary, body);
  item.append(details);
  return item;
}

function paintCount() {
  setText('log-badge', formatNumber(total));
  setHidden('log-empty', total > 0);
}

function scrollEnd() {
  const scroller = el('log-scroll');
  if (scroller) scroller.scrollTop = scroller.scrollHeight;
}

export function logPackets(packets) {
  if (!Array.isArray(packets) || packets.length === 0) return;
  const list = el('packet-log');
  if (!list) return;

  for (const packet of packets) {
    list.append(buildRow(packet));
    total += 1;
  }
  while (list.children.length > 500) list.firstElementChild.remove();
  paintCount();
  if (follow) scrollEnd();
}

export function clearLog() {
  const list = el('packet-log');
  if (list) while (list.firstChild) list.removeChild(list.firstChild);
  total = 0;
  paintCount();
  const scroller = el('log-scroll');
  if (scroller) scroller.scrollTop = 0;
}

export function setLogLive(connected) {
  setText('log-live-word', connected ? 'LIVE' : 'HOLD');
  const dot = el('log-live-dot');
  if (dot) dot.className = 'lamp' + (connected ? ' on' : '');
}

function setOpen(open) {
  const drawer = el('log-drawer');
  const toggle = el('btn-log-toggle');
  if (!drawer || !toggle) return;
  drawer.hidden = !open;
  toggle.classList.toggle('on', open);
  toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
  if (open && follow) scrollEnd();
}

export function bindLogDrawer() {
  const toggle = el('btn-log-toggle');
  const close = el('btn-log-close');
  const clear = el('btn-clear-log');
  const followBtn = el('btn-log-follow');

  if (toggle) toggle.addEventListener('click', () => {
    const drawer = el('log-drawer');
    setOpen(Boolean(drawer && drawer.hidden));
  });
  if (close) close.addEventListener('click', () => setOpen(false));
  if (clear) clear.addEventListener('click', clearLog);
  if (followBtn) followBtn.addEventListener('click', () => {
    follow = !follow;
    setText('log-follow-word', follow ? 'ON' : 'OFF');
    if (follow) scrollEnd();
  });

  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') setOpen(false);
  });

  paintCount();
  setLogLive(false);
}
