// Boot: verify the bridge, bind the page, then take one daemon snapshot.
// No device is opened and no packet is sent from startup.

import { hasBridge } from './bridge.js';
import { bindDaemon } from './daemon.js';
import { el, setHidden, setText } from './dom.js';
import { bindImage } from './image.js';
import { bindLogDrawer } from './log.js';
import { bindPreviewInputs, renderPreview } from './preview.js';
import { bindRaw } from './raw.js';
import { bindSections } from './sections.js';
import { bindTelemetry } from './telemetry.js';

function bindModes() {
  const body = el('device-body');
  const buttons = Array.from(document.querySelectorAll('#mode-seg button[data-mode]'));
  if (!body || buttons.length === 0) return;

  const select = (button) => {
    const mode = button.dataset.mode;
    body.dataset.mode = mode;
    for (const other of buttons) {
      other.setAttribute('aria-pressed', other === button ? 'true' : 'false');
    }
    renderPreview();
  };

  for (const button of buttons) {
    button.addEventListener('click', () => select(button));
    button.addEventListener('keydown', (event) => {
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
      event.preventDefault();
      const index = buttons.indexOf(button);
      const next = (index + (event.key === 'ArrowRight' ? 1 : buttons.length - 1)) % buttons.length;
      buttons[next].focus();
      select(buttons[next]);
    });
  }
}

function init() {
  bindLogDrawer();

  if (!hasBridge()) {
    document.documentElement.dataset.mystiqueUi = 'no-bridge';
    setText('app-error', 'Tauri IPC is unavailable. Launch this UI through mystique-ui.');
    setHidden('app-error', false);
    return;
  }

  bindModes();
  bindSections();
  bindRaw();
  bindTelemetry();
  bindImage();
  bindPreviewInputs();
  document.documentElement.dataset.mystiqueUi = 'ready';

  // Listeners first, snapshot second: the daemon does not replay events.
  void bindDaemon();
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}
