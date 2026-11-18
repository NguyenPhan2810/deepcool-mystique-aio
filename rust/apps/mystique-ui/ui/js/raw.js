// The raw packet panel. The daemon builds and checksums the 48-byte packet; this
// module only parses the owner's hex, calls the command, and appends the exact record
// to the packet log. No example bytes are ever loaded.

import { runCommandDetailed } from './bridge.js';
import { el, setDisabled, setHidden, setText } from './dom.js';
import { logPackets } from './log.js';

function clearOutput() {
  setText('raw-packet', '—');
  setText('raw-result', '—');
  setText('raw-reply', '');
  setText('raw-status', '');
  setHidden('raw-error', true);
  setText('raw-error', '');
}

function parseCommand(text) {
  const cleaned = String(text || '').trim().replace(/^0x/i, '');
  if (!/^[0-9a-fA-F]{1,2}$/.test(cleaned)) {
    throw new Error('command must be one or two hex digits, 00–FF');
  }
  return Number.parseInt(cleaned, 16);
}

function parsePayload(text) {
  const raw = String(text || '').trim();
  const cleaned = raw.replace(/0x/gi, '').replace(/[\s,]+/g, '');
  if (cleaned.length === 0) {
    if (raw.length > 0) throw new Error('payload has no hex digits');
    return [];
  }
  if (!/^[0-9a-fA-F]+$/.test(cleaned)) throw new Error('payload must be hex bytes');
  if (cleaned.length % 2 !== 0) throw new Error('payload needs an even number of hex digits');
  const bytes = [];
  for (let index = 0; index < cleaned.length; index += 2) {
    bytes.push(Number.parseInt(cleaned.slice(index, index + 2), 16));
  }
  if (bytes.length > 39) throw new Error('payload is ' + bytes.length + ' bytes; the maximum is 39');
  return bytes;
}

function showLocalError(message) {
  const node = el('raw-error');
  if (!node) return;
  node.textContent = message;
  node.title = message;
  node.hidden = false;
}

async function run(dryRun) {
  clearOutput();
  let command;
  let payload;
  let pipe;
  try {
    command = parseCommand(el('raw-command') ? el('raw-command').value : '');
    payload = parsePayload(el('raw-payload') ? el('raw-payload').value : '');
    pipe = el('raw-pipe') ? el('raw-pipe').value : '';
    if (!pipe) throw new Error('choose the init or data pipe');
  } catch (error) {
    showLocalError(error.message || String(error));
    return;
  }

  setDisabled('btn-raw-dry', true);
  setDisabled('btn-raw-send', true);
  const result = await runCommandDetailed(
    'raw_packet',
    { command, payload, pipe, dry_run: dryRun },
    'raw',
  );
  setDisabled('btn-raw-dry', false);
  setDisabled('btn-raw-send', false);
  const outcome = result.outcome;
  if (!outcome) return;

  setText('raw-packet', outcome.packet_hex || '—');
  if (!outcome.sent) {
    setText('raw-result', 'Dry run · not sent');
    setText('raw-status', 'NOT SENT');
    return;
  }

  const code = Number(outcome.result_code);
  const resultWord = '0x' + (Number.isFinite(code) ? code.toString(16).toUpperCase().padStart(2, '0') : '??');
  setText('raw-result', resultWord + ' ' + (outcome.result || 'unknown'));
  setText('raw-reply', outcome.reply_hex || '');
  setText('raw-status', 'SENT · LOGGED');
  if (outcome.record) logPackets([outcome.record]);
}

export function bindRaw() {
  const dry = el('btn-raw-dry');
  const send = el('btn-raw-send');
  if (dry) dry.addEventListener('click', () => { void run(true); });
  if (send) send.addEventListener('click', () => { void run(false); });
  clearOutput();
}
