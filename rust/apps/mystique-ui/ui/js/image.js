// Image/GIF upload. The file is chosen locally, the plan comes from the daemon,
// and the send waits for a confirm. The plan is never invented in the UI.

import { chooseImage, runCommand } from './bridge.js';
import { clearError, showError } from './errors.js';
import { el, formatNumber, setDisabled, setHidden, setText, setTitle } from './dom.js';
import { applyOutcome, state } from './state.js';
import { renderPreview } from './preview.js';

function currentFit() {
  const select = el('upload-fit');
  return select ? select.value : 'cover';
}

function shortName(path) {
  return String(path || '').split(/[\\/]/).pop();
}

function renderPlan(plan) {
  setText('upload-plan-kind', plan.kind === 'gif' ? 'GIF' : 'Image');
  setText(
    'upload-plan-size',
    plan.source_width + ' × ' + plan.source_height + ' → ' + plan.output_width + ' × ' + plan.output_height,
  );
  setText('upload-plan-frames', formatNumber(plan.frames));
  setText('upload-plan-bytes', formatNumber(plan.total_bytes));
  setText('upload-plan-chunks', formatNumber(plan.chunk_count));
  setText('upload-plan-commands', formatNumber(plan.command_count));
  setText('upload-plan-content-id', plan.content_id);

  setText('upload-confirm-copy', 'Plan ready.');

  const preview = el('upload-preview');
  if (preview && plan.preview) {
    preview.src = plan.preview;
    setText('upload-preview-note', 'FRAME 1');
    setHidden('upload-preview-box', false);
  } else if (preview) {
    preview.removeAttribute('src');
    setHidden('upload-preview-box', true);
  }

  const list = el('upload-plan-command-list');
  if (!list) return;
  while (list.firstChild) list.removeChild(list.firstChild);
  for (const command of plan.commands) {
    const item = document.createElement('li');
    item.textContent = command.label + ' · ' + command.pipe + ' · ' + (command.payload_hex || 'empty');
    list.append(item);
  }
}

function resetPlan() {
  state.plan = null;
  state.planPath = null;
  state.planFit = null;
  state.mediaPreview = null;
  state.mediaKind = null;
  setHidden('upload-plan', true);
  setHidden('upload-preview-box', true);
  setDisabled('btn-upload', true);
  const confirm = el('upload-confirm');
  if (confirm) confirm.checked = false;
  renderPreview();
}

function bindPicker() {
  const pick = el('btn-pick-image');
  if (!pick) return;
  pick.addEventListener('click', async () => {
    clearError('upload');
    try {
      const selected = await chooseImage();
      if (typeof selected !== 'string' || selected.length === 0) return;
      state.selectedPath = selected;
      setText('image-path', shortName(selected));
      setTitle('image-path', selected);
      setText('upload-message', '');
      resetPlan();
      // The plan is local arithmetic: build it as soon as there is a file. Uploading is
      // the irreversible step and stays behind its own confirm.
      await buildPlan();
    } catch (error) {
      showError('upload', error);
    }
  });
}

async function buildPlan() {
  if (!state.selectedPath) return;
  resetPlan();
  const fit = currentFit();
  const plan = await runCommand('plan_upload', { path: state.selectedPath, fit }, 'upload');
  if (!plan) return;
  state.plan = plan;
  state.planPath = state.selectedPath;
  state.planFit = fit;
  state.mediaPreview = plan.preview || null;
  state.mediaKind = plan.kind;
  renderPlan(plan);
  setHidden('upload-plan', false);
  setText('upload-message', '');
  renderPreview();
}

function bindFit() {
  const fit = el('upload-fit');
  if (!fit) return;
  fit.addEventListener('change', () => { void buildPlan(); });
}

function planIsCurrent() {
  return Boolean(
    state.plan && state.selectedPath &&
      state.planPath === state.selectedPath && state.planFit === currentFit(),
  );
}

function bindConfirm() {
  const confirm = el('upload-confirm');
  if (!confirm) return;
  confirm.addEventListener('change', () => {
    setDisabled('btn-upload', !(confirm.checked && planIsCurrent()));
  });
}

function bindSend() {
  const upload = el('btn-upload');
  if (!upload) return;
  upload.addEventListener('click', async () => {
    if (state.uploading || !planIsCurrent()) return;
    const confirm = el('upload-confirm');
    if (!confirm || !confirm.checked) return;

    state.uploading = true;
    setDisabled('btn-upload', true);
    setDisabled('btn-pick-image', true);
    setText('upload-message', 'sending · stream stops first');

    const command = state.plan.kind === 'gif' ? 'upload_gif' : 'upload_image';
    const outcome = await runCommand(
      command,
      { path: state.selectedPath, fit: currentFit() },
      'upload',
    );

    state.uploading = false;
    setDisabled('btn-pick-image', false);

    if (!outcome) {
      setText('upload-message', '');
      setDisabled('btn-upload', !confirm.checked);
      return;
    }

    applyOutcome(outcome);
    setText('upload-message', outcome.message || 'uploaded · stream stopped');
    confirm.checked = false;
    setDisabled('btn-upload', true);
    renderPreview();
  });
}

export function bindImage() {
  bindPicker();
  bindFit();
  bindConfirm();
  bindSend();
}
