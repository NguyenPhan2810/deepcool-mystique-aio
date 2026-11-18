// The only module that talks to Tauri. It also owns the failure path: show the error
// inline in the calling section and log any packets that were attempted before it.
//
// The file dialog is a plugin command. It is invoked through the core bridge rather
// than `window.__TAURI__.dialog`, because the plugin's global IIFE is not guaranteed to
// be present when `withGlobalTauri` is used; the command and its ACL permission are.
// This is exactly the call the plugin's own JS wrapper makes.

import { clearError, showError } from './errors.js';
import { logPackets } from './log.js';

const tauri = window.__TAURI__;
const core = tauri && tauri.core ? tauri.core : null;

const invoke = core && typeof core.invoke === 'function' ? core.invoke.bind(core) : null;

export function hasBridge() {
  return Boolean(invoke);
}

export async function runCommand(name, args, section) {
  const result = await runCommandDetailed(name, args, section);
  return result.outcome;
}

// Like runCommand, but also returns the packets that were attempted before a failure.
// A sequence that fails halfway still has to be recorded exactly, or a negative
// hardware result is ambiguous.
export async function runCommandDetailed(name, args, section) {
  if (!invoke) {
    showError(section, 'Tauri IPC is unavailable; the webview did not receive the bridge.');
    return { outcome: null, packets: [] };
  }
  clearError(section);
  try {
    return { outcome: await invoke(name, args || {}), packets: [] };
  } catch (error) {
    showError(section, error);
    const packets = error && Array.isArray(error.packets) ? error.packets : [];
    if (packets.length > 0) logPackets(packets);
    return { outcome: null, packets };
  }
}

// A read-only poll: no section error is cleared or shown, so a transient failure while
// polling cannot flicker the message the owner is reading. Returns null instead.
export async function readCommand(name, args) {
  if (!invoke) return null;
  try {
    return await invoke(name, args || {});
  } catch {
    return null;
  }
}

export async function chooseImage() {
  if (!invoke) throw new Error('Tauri IPC is unavailable');
  return invoke('plugin:dialog|open', {
    options: {
      title: 'Choose an image or GIF',
      multiple: false,
      directory: false,
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif'] }],
    },
  });
}

// Daemon events. Same bridge, same rule: nothing else imports `__TAURI__`.
// Resolves to an unlisten function, or null when the event API is not present.
export function listen(name, handler) {
  const events = tauri && tauri.event ? tauri.event : null;
  if (!events || typeof events.listen !== 'function') return Promise.resolve(null);
  return events.listen(name, (event) => handler(event.payload));
}

