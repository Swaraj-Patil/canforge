// Entry point: loads the WebAssembly module and the example bus, switches
// tabs, and opens the files people choose or drop on the page.
import { loadCanforge } from './canforge.js';
import { $, fetchText, readChosenFile, setStatus, state } from './state.js';
import { prefixFrom } from './format.js';
import { bindFrames, renderMessage, renderMessageList, renderSummary } from './views/frames.js';
import { bindSearch, clearSearch } from './views/search.js';
import { renderProblems } from './views/problems.js';
import { bindCode, renderCode } from './views/code.js';
import { bindCompare, useExampleRevisions } from './views/compare.js';

// Parses the file once, in WebAssembly, which keeps the result for decoding
// and code generation. A file that does not parse leaves the open one loaded.
function loadDatabase(text, fileName, isExample) {
  const started = performance.now();
  const analysis = state.cf.load(text);
  const loadMs = performance.now() - started;
  if (!analysis.ok) {
    const where = analysis.error.line ? ` (line ${analysis.error.line})` : '';
    setStatus(`${fileName} could not be read${where}: ${analysis.error.message}.`, true);
    return false;
  }
  Object.assign(state, {
    src: text,
    fileName,
    isExample,
    analysis,
    loadMs,
    selected: 0,
    inspect: null,
    frames: new Map(),
    animate: true,
    fileIndex: 0,
  });
  $('prefix').value = prefixFrom(fileName);
  setStatus('');
  renderSummary();
  clearSearch();
  renderMessageList();
  renderMessage();
  renderProblems();
  if (!$('view-code').hidden) renderCode();
  return true;
}

async function useExample() {
  try {
    loadDatabase(await fetchText('examples/powertrain.dbc'), 'powertrain.dbc', true);
  } catch (err) {
    setStatus(`The example bus did not load: ${err.message}.`, true);
  }
}

const TABS = ['frames', 'problems', 'code', 'compare'];

function showTab(name) {
  for (const t of TABS) {
    const selected = t === name;
    $(`tab-${t}`).setAttribute('aria-selected', String(selected));
    $(`tab-${t}`).tabIndex = selected ? 0 : -1;
    $(`view-${t}`).hidden = !selected;
  }
  if (name === 'code') renderCode();
  if (name === 'compare' && !state.oldRev && !state.newRev && state.cf) useExampleRevisions();
}

function bindUi() {
  for (const t of TABS) {
    $(`tab-${t}`).addEventListener('click', () => showTab(t));
    $(`tab-${t}`).addEventListener('keydown', (e) => {
      if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
      const next = TABS[(TABS.indexOf(t) + (e.key === 'ArrowRight' ? 1 : TABS.length - 1)) % TABS.length];
      showTab(next);
      $(`tab-${next}`).focus();
    });
  }

  $('use-example').addEventListener('click', () => {
    if (state.cf) useExample().then(() => showTab('frames'));
  });

  $('file-input').addEventListener('change', async (e) => {
    const file = await readChosenFile(e.target);
    if (file && state.cf && loadDatabase(file.text, file.name, false)) showTab('frames');
  });

  document.addEventListener('dragover', (e) => e.preventDefault());
  document.addEventListener('drop', async (e) => {
    e.preventDefault();
    const file = e.dataTransfer && e.dataTransfer.files[0];
    if (file && state.cf && loadDatabase(await file.text(), file.name, false)) showTab('frames');
  });

  bindFrames();
  bindSearch({ showFrames: () => showTab('frames') });
  bindCode();
  bindCompare();
}

async function main() {
  bindUi();
  setStatus('Loading canforge.');
  try {
    state.cf = await loadCanforge('canforge.wasm');
  } catch (err) {
    setStatus(
      `canforge.wasm did not load (${err.message}). This page runs the WebAssembly build that CI publishes; see the README to build it yourself.`,
      true,
    );
    return;
  }
  $('version').textContent = state.cf.version();
  await useExample();
}

main();
