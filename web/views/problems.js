// Problems view: the lint findings for the open file.
import { $, state } from '../state.js';
import { escapeHtml, plural, sentence } from '../format.js';

export function renderProblems() {
  const el = $('problems');
  const a = state.analysis;
  if (!a) {
    el.innerHTML = '';
    return;
  }
  if (!a.diagnostics.length) {
    el.innerHTML = `<p class="problem-summary">No problems found in ${escapeHtml(state.fileName)}.</p>`;
    return;
  }
  const s = a.summary;
  const parts = [];
  if (s.errors) parts.push(plural(s.errors, 'error'));
  if (s.warnings) parts.push(plural(s.warnings, 'warning'));
  if (s.infos) parts.push(plural(s.infos, 'note'));
  const names = { error: 'Error', warning: 'Warning', info: 'Note' };
  const items = a.diagnostics
    .map(
      (d) => `<li class="${escapeHtml(d.severity)}"><span class="sev">${names[d.severity] || escapeHtml(d.severity)}</span>` +
        `<span class="rule">${escapeHtml(d.rule)} ${escapeHtml(d.name)}</span><span class="where">Line ${d.line}</span>` +
        `<p>${escapeHtml(sentence(d.message))}</p></li>`,
    )
    .join('');
  el.innerHTML = `<p class="problem-summary">${escapeHtml(parts.join(', '))} in ${escapeHtml(state.fileName)}</p><ol class="problems">${items}</ol>`;
}
