// Compare view: which changes between two revisions of a DBC file would
// make existing decoders misread frames.
import { $, fetchText, readChosenFile, state } from '../state.js';
import { escapeHtml, sentence } from '../format.js';

export async function useExampleRevisions() {
  try {
    const [a, b] = await Promise.all([fetchText('examples/v1.dbc'), fetchText('examples/v2.dbc')]);
    state.oldRev = { name: 'v1.dbc', text: a };
    state.newRev = { name: 'v2.dbc', text: b };
    renderCompare();
  } catch (err) {
    $('compare-result').innerHTML = `<p class="status error">${escapeHtml(sentence(err.message))}</p>`;
  }
}

function renderCompare() {
  const o = state.oldRev;
  const n = state.newRev;
  $('compare-files').textContent = o || n ? `Old: ${o ? o.name : 'not chosen yet'}. New: ${n ? n.name : 'not chosen yet'}.` : '';
  const out = $('compare-result');
  if (!o || !n) {
    out.innerHTML = '';
    return;
  }
  const r = state.cf.diff(o.text, n.text);
  if (!r.ok) {
    out.innerHTML = `<p class="status error">${escapeHtml(sentence(r.error.message))}</p>`;
    return;
  }
  const headlines = {
    breaking: 'Breaking: existing decoders would misread some frames.',
    caution: 'Needs review: the wire format is unchanged, but names, ranges or routing changed.',
    compatible: 'Compatible: existing decoders keep working.',
    identical: 'The two revisions are identical.',
  };
  const groups = [
    ['breaking', 'Breaking changes'],
    ['caution', 'Needs review'],
    ['compatible', 'Compatible changes'],
  ];
  const c = r.counts;
  out.innerHTML =
    `<p class="verdict ${escapeHtml(r.verdict)}">${headlines[r.verdict] || escapeHtml(r.verdict)}</p>` +
    `<p class="muted">${c.breaking} breaking, ${c.caution} needing review, ${c.compatible} compatible.</p>` +
    groups
      .map(([level, title]) => {
        const items = r.changes.filter((x) => x.level === level);
        if (!items.length) return '';
        return `<div class="change-group"><h3>${title}</h3><ul>${items.map((x) => `<li>${escapeHtml(x.message)}</li>`).join('')}</ul></div>`;
      })
      .join('');
}

export function bindCompare() {
  $('old-file').addEventListener('change', async (e) => {
    state.oldRev = (await readChosenFile(e.target)) || state.oldRev;
    renderCompare();
  });
  $('new-file').addEventListener('change', async (e) => {
    state.newRev = (await readChosenFile(e.target)) || state.newRev;
    renderCompare();
  });
  $('example-diff').addEventListener('click', () => {
    if (state.cf) useExampleRevisions();
  });
}
