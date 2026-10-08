// Entry point: loads the WebAssembly module, opens the right file (a link to
// the example bus, the file this browser kept, or the example), switches
// tabs, and opens the files people choose or drop on the page.
import { loadCanforge } from './canforge.js';
import { $, currentMessage, fetchText, readChosenFile, setStatus, state } from './state.js';
import { escapeHtml, parseHexInput, prefixFrom } from './format.js';
import { TABS, noteOpeningView, openingView, readLink, writeLink } from './link.js';
import { forgetFile, savedFile, saveFile } from './remember.js';
import { bindFrames, renderMessage, renderMessageList, renderSummary, selectMessage, setCurrentBytes } from './views/frames.js';
import { bindSearch, clearSearch } from './views/search.js';
import { renderProblems } from './views/problems.js';
import { bindCode, renderCode } from './views/code.js';
import { bindCompare, useExampleRevisions } from './views/compare.js';

function readProblem(fileName, error) {
  const where = error.line ? ` (line ${error.line})` : '';
  return `${fileName} could not be read${where}: ${error.message}.`;
}

// Parses the file once, in WebAssembly, which keeps the result for decoding
// and code generation. Returns the parse error, or null once the file is
// open. A file that does not parse leaves the open one loaded.
function loadDatabase(text, fileName, isExample) {
  const started = performance.now();
  const analysis = state.cf.load(text);
  const loadMs = performance.now() - started;
  if (!analysis.ok) return analysis.error;
  openingView();
  Object.assign(state, {
    src: text,
    fileName,
    isExample,
    analysis,
    loadMs,
    stored: null,
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
  renderStorageNote();
  noteOpeningView();
  writeLink({ now: true });
  return null;
}

async function useExample() {
  try {
    const error = loadDatabase(await fetchText('examples/powertrain.dbc'), 'powertrain.dbc', true);
    if (error) setStatus(readProblem('powertrain.dbc', error), true);
  } catch (err) {
    setStatus(`The example bus did not load: ${err.message}.`, true);
  }
}

// A file someone chose or dropped: open it, and keep a copy in this browser.
function openFile(file) {
  const error = loadDatabase(file.text, file.name, false);
  if (error) {
    setStatus(readProblem(file.name, error), true);
    return;
  }
  state.stored = saveFile(file.name, file.text, file.size);
  renderStorageNote();
  showTab('frames');
}

// Opens the copy this browser kept. Returns null once it is open, or the
// reason it did not open.
function openSaved(saved) {
  const error = loadDatabase(saved.text, saved.name, false);
  if (!error) {
    state.stored = 'saved';
    renderStorageNote();
  }
  return error;
}

function forgetBroken(saved, error) {
  forgetFile();
  renderStorageNote();
  setStatus(`${readProblem(saved.name, error)} canforge removed the copy this browser kept.`, true);
}

// Says what this browser keeps, so nothing is stored without the page saying so.
function renderStorageNote() {
  const note = $('storage-note');
  let html = '';
  if (!state.isExample) {
    const texts = {
      saved: 'This browser keeps a copy so the file opens next time. The copy never leaves your computer.',
      'too-large': 'canforge keeps files of up to 2 MB, so this one will not open next time.',
      refused: 'This browser refused to keep a copy, so the file will not open next time.',
      forgotten: 'This browser no longer keeps a copy of the file.',
    };
    if (texts[state.stored]) html = `<p>${texts[state.stored]}</p>`;
    if (state.stored === 'saved') html += '<button type="button" class="link-button" data-storage="forget">Forget this file</button>';
  } else {
    const saved = savedFile();
    if (saved) {
      html =
        `<p>This browser keeps your last file, ${escapeHtml(saved.name)}, and opens it next time.</p>` +
        '<button type="button" class="link-button" data-storage="open">Open it</button> ' +
        '<button type="button" class="link-button" data-storage="forget">Forget it</button>';
    }
  }
  note.innerHTML = html;
  note.hidden = !html;
}

// Shows the view a link asks for on the example bus, and says what it could not show.
function applyLink(link) {
  const problems = [];
  if (link.message) {
    const index = state.analysis.messages.findIndex((m) => m.name === link.message);
    if (index >= 0) selectMessage(index);
    else problems.push(`The link names a message the example bus does not have, ${link.message}, so the first message is shown.`);
  }
  if (link.bytes) {
    const parsed = parseHexInput(link.bytes, currentMessage().dlc);
    if (parsed.bytes) setCurrentBytes(parsed.bytes);
    else problems.push(`The link's bytes were left out: ${parsed.error}`);
  }
  if (TABS.includes(link.tab)) showTab(link.tab);
  else if (link.tab) problems.push(`The link names a tab that does not exist, ${link.tab}.`);
  if (problems.length) setStatus(problems.join(' '), true);
}

function showTab(name) {
  state.tab = name;
  for (const t of TABS) {
    const selected = t === name;
    $(`tab-${t}`).setAttribute('aria-selected', String(selected));
    $(`tab-${t}`).tabIndex = selected ? 0 : -1;
    $(`view-${t}`).hidden = !selected;
  }
  if (name === 'code') renderCode();
  if (name === 'compare' && !state.oldRev && !state.newRev && state.cf) useExampleRevisions();
  writeLink({ now: true });
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
    if (file && state.cf) openFile(file);
  });

  document.addEventListener('dragover', (e) => e.preventDefault());
  document.addEventListener('drop', async (e) => {
    e.preventDefault();
    const file = e.dataTransfer && e.dataTransfer.files[0];
    if (file && state.cf) openFile({ name: file.name, text: await file.text(), size: file.size });
  });

  $('storage-note').addEventListener('click', (e) => {
    const action = e.target.closest('[data-storage]');
    if (!action) return;
    const saved = savedFile();
    if (action.dataset.storage === 'forget') {
      forgetFile();
      state.stored = state.isExample ? null : 'forgotten';
      renderStorageNote();
    } else if (saved) {
      const error = openSaved(saved);
      if (error) forgetBroken(saved, error);
      else showTab('frames');
    }
  });

  // A link pasted into the address bar while the page is open.
  window.addEventListener('hashchange', async () => {
    const link = readLink();
    if (!link || !state.cf) return;
    if (!state.isExample) await useExample();
    applyLink(link);
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
  // A link opens the example bus; otherwise the file this browser kept opens.
  const link = readLink();
  const saved = link ? null : savedFile();
  const error = saved ? openSaved(saved) : null;
  if (saved && !error) return;
  await useExample();
  if (link) applyLink(link);
  if (error) forgetBroken(saved, error);
}

main();
