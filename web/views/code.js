// Generated code view: C or Python for the open file, to copy or download.
import { $, flash, state } from '../state.js';
import { escapeHtml, plural, sentence } from '../format.js';

function setCodeButtons(enabled) {
  $('copy-code').disabled = !enabled;
  $('download-code').disabled = !enabled;
}

export function renderCode() {
  if (!state.analysis) return;
  const result = state.cf.generate(state.src, state.lang, $('prefix').value.trim(), state.fileName);
  if (!result.ok) {
    state.files = [];
    $('code-files').innerHTML = '';
    $('code-note').textContent = sentence(result.error.message);
    $('code-output').textContent = '';
    setCodeButtons(false);
    return;
  }
  state.files = result.files;
  if (state.fileIndex >= state.files.length) state.fileIndex = 0;
  $('code-files').innerHTML = state.files
    .map((f, i) => `<button type="button" data-file="${i}" aria-pressed="${i === state.fileIndex}">${escapeHtml(f.name)}</button>`)
    .join('');
  const file = state.files[state.fileIndex];
  const lines = file.content.split('\n').length;
  $('code-note').textContent =
    state.lang === 'c'
      ? `C99, no heap allocation, no global state, warning-free under strict GCC flags. ${plural(lines, 'line')}.`
      : `Plain Python with no dependencies. ${plural(lines, 'line')}.`;
  $('code-output').textContent = file.content;
  setCodeButtons(true);
}

export function bindCode() {
  for (const button of document.querySelectorAll('.segmented button')) {
    button.addEventListener('click', () => {
      state.lang = button.dataset.lang;
      state.fileIndex = 0;
      for (const b of document.querySelectorAll('.segmented button')) b.setAttribute('aria-pressed', String(b === button));
      renderCode();
    });
  }
  $('prefix').addEventListener('input', () => renderCode());
  $('code-files').addEventListener('click', (e) => {
    const button = e.target.closest('button[data-file]');
    if (!button) return;
    state.fileIndex = Number(button.dataset.file);
    renderCode();
  });
  $('copy-code').addEventListener('click', async (e) => {
    const file = state.files[state.fileIndex];
    if (!file) return;
    try {
      await navigator.clipboard.writeText(file.content);
      flash(e.target, 'Copied');
    } catch {
      flash(e.target, 'Copy blocked');
    }
  });
  $('download-code').addEventListener('click', () => {
    const file = state.files[state.fileIndex];
    if (!file) return;
    const url = URL.createObjectURL(new Blob([file.content], { type: 'text/plain' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = file.name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
}
