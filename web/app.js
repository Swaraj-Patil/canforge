import { loadCanforge } from './canforge.js';

// Signal colors borrowed from wiring-harness insulation.
const PALETTE = ['#D9692B', '#2E8B57', '#3567C8', '#C79A10', '#7B55C9', '#C23F37', '#1C8A8A', '#8B5E3C', '#C24D93', '#5E7A1F', '#2C4A9A', '#6E7C87'];

// Plausible frames for the example bus, so the first view shows real values.
const EXAMPLE_FRAMES = {
  VehicleStatus: 'e8 03 03 5a 18 fc 00 05',
  BatteryStatus: '9c 40 ff 06 a0 1f b8 00',
  InverterTelemetry: '00 80 0c e2 04 00 00 02',
  ChargerLimits: '40 01 a0 0f 01 00 00 00',
  ThermalSensors: '00 00 48 41 00 00 ac 41',
  WheelSpeeds: '20 b2 09 20 82 0a 96 c0',
  DiagnosticFD: '40 e2 01 00 07 00 00 00 34 12 ee ff c0 00 00 00',
};

const $ = (id) => document.getElementById(id);

const state = {
  cf: null,
  src: '',
  fileName: '',
  isExample: false,
  analysis: null,
  selected: 0,
  frames: new Map(),
  animate: true,
  lang: 'c',
  fileIndex: 0,
  files: [],
  oldRev: null,
  newRev: null,
};

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

function sentence(text) {
  const t = String(text);
  return t.charAt(0).toUpperCase() + t.slice(1) + (/[.!?]$/.test(t) ? '' : '.');
}

function plural(n, word) {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

function setStatus(text, isError = false) {
  const el = $('status');
  el.textContent = text;
  el.classList.toggle('error', isError);
}

function colorFor(index) {
  return PALETTE[index % PALETTE.length];
}

function tint(hex, alpha) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

function toHex(bytes) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join(' ');
}

function parseHexInput(text, length) {
  const tokens = text.split(/[\s:,_-]+/).filter(Boolean);
  const out = [];
  for (let token of tokens) {
    if (/^0x/i.test(token)) token = token.slice(2);
    if (!/^[0-9a-f]+$/i.test(token)) return { error: `"${token}" is not hexadecimal.` };
    if (token.length === 1) token = '0' + token;
    if (token.length % 2 !== 0) return { error: `"${token}" has an odd number of digits.` };
    for (let i = 0; i < token.length; i += 2) out.push(parseInt(token.slice(i, i + 2), 16));
  }
  if (out.length > length) return { error: `This frame holds ${plural(length, 'byte')}; you entered ${out.length}.` };
  const bytes = new Uint8Array(length);
  bytes.set(out);
  return { bytes };
}

function decimalsOf(x) {
  if (!Number.isFinite(x) || Number.isInteger(x)) return 0;
  const s = Math.abs(x).toString();
  const exp = s.match(/e-(\d+)$/);
  if (exp) {
    const mantissa = s.split('e')[0];
    return parseInt(exp[1], 10) + (mantissa.includes('.') ? mantissa.split('.')[1].length : 0);
  }
  return s.includes('.') ? s.split('.')[1].length : 0;
}

function formatPhysical(sig, value) {
  if (value === null) return 'not a number';
  if (sig.value_type !== 'integer') return String(Number(value.toPrecision(7)));
  const d = Math.min(12, Math.max(decimalsOf(sig.factor), decimalsOf(sig.offset)));
  return value.toFixed(d);
}

function prefixFrom(fileName) {
  const stem = fileName.replace(/\.[^.]*$/, '');
  const snake = stem
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/[^A-Za-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toLowerCase();
  if (!snake) return 'canbus';
  return /^[0-9]/.test(snake) ? `x_${snake}` : snake;
}

function currentMessage() {
  return state.analysis ? state.analysis.messages[state.selected] || null : null;
}

function frameFor(msg) {
  let bytes = state.frames.get(msg.name);
  if (!bytes) {
    bytes = new Uint8Array(msg.dlc);
    const sample = state.isExample ? EXAMPLE_FRAMES[msg.name] : null;
    if (sample) {
      const parsed = parseHexInput(sample, msg.dlc);
      if (parsed.bytes) bytes.set(parsed.bytes);
    }
    state.frames.set(msg.name, bytes);
  }
  return bytes;
}

function writeRaw(bytes, sig, value) {
  const v = BigInt.asUintN(Math.max(sig.length, 1), BigInt(value));
  sig.bits.forEach(([byte, bit], rawBit) => {
    if (byte >= bytes.length) return;
    if ((v >> BigInt(rawBit)) & 1n) bytes[byte] |= 1 << bit;
    else bytes[byte] &= ~(1 << bit) & 0xff;
  });
}

// ---------------------------------------------------------------- loading

function loadDatabase(text, fileName, isExample) {
  const analysis = state.cf.analyze(text);
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
    selected: 0,
    frames: new Map(),
    animate: true,
    fileIndex: 0,
  });
  $('prefix').value = prefixFrom(fileName);
  setStatus('');
  renderSummary();
  renderMessageList();
  renderMessage();
  renderProblems();
  if (!$('view-code').hidden) renderCode();
  return true;
}

async function fetchText(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path} returned HTTP ${response.status}`);
  return response.text();
}

async function useExample() {
  try {
    loadDatabase(await fetchText('examples/powertrain.dbc'), 'powertrain.dbc', true);
  } catch (err) {
    setStatus(`The example bus did not load: ${err.message}.`, true);
  }
}

async function readChosenFile(input) {
  const file = input.files && input.files[0];
  input.value = '';
  if (!file) return null;
  return { name: file.name, text: await file.text() };
}

// ---------------------------------------------------------------- frames view

function renderSummary() {
  const s = state.analysis.summary;
  $('db-summary').innerHTML =
    `<h2>${escapeHtml(state.fileName)}</h2>` +
    `<p>${plural(s.messages, 'message')}, ${plural(s.signals, 'signal')}, ${plural(s.nodes, 'node')}</p>`;
  const total = s.errors + s.warnings + s.infos;
  $('problem-count').textContent = total ? String(total) : '';
}

function barcode(msg) {
  const width = Math.max(msg.dlc * 8, 1);
  const rects = [];
  msg.signals.forEach((sig, si) => {
    for (const [byte, bit] of sig.bits) {
      if (byte >= msg.dlc) continue;
      rects.push(`<rect x="${byte * 8 + (7 - bit)}" y="0" width="1" height="1" fill="${colorFor(si)}"/>`);
    }
  });
  return `<svg class="barcode" viewBox="0 0 ${width} 1" preserveAspectRatio="none" aria-hidden="true">${rects.join('')}</svg>`;
}

function renderMessageList() {
  const messages = state.analysis.messages;
  $('message-list').innerHTML = messages
    .map(
      (msg, i) => `<li><button type="button" data-index="${i}" aria-current="${i === state.selected}">
        <span class="name">${escapeHtml(msg.name)}</span>
        <span class="meta">${escapeHtml(msg.id_hex)}, ${plural(msg.dlc, 'byte')}</span>
        ${barcode(msg)}</button></li>`,
    )
    .join('');
}

function renderMessage() {
  const detail = $('message-detail');
  const msg = currentMessage();
  if (!msg) {
    detail.innerHTML = '<p class="muted">This file defines no messages.</p>';
    return;
  }
  const facts = [`Frame ${msg.id_hex}`, msg.extended ? 'extended 29-bit ID' : 'standard 11-bit ID', plural(msg.dlc, 'byte')];
  if (msg.sender && msg.sender !== 'Vector__XXX') facts.push(`sent by ${msg.sender}`);
  detail.innerHTML = `
    <h2>${escapeHtml(msg.name)}</h2>
    <p class="facts">${escapeHtml(facts.join(', '))}.</p>
    ${msg.comment ? `<p class="comment">${escapeHtml(msg.comment)}</p>` : ''}
    <div class="frame-area">
      <div class="matrix-wrap">
        <p class="hint">Each square is one bit of the frame. Click a bit to flip it and watch the decoded values change.</p>
        <div id="pages"></div>
        <div class="matrix" id="matrix" role="group" aria-label="Bits of ${escapeHtml(msg.name)}"></div>
        <p class="legend-note">Corner marks show each signal's most significant bit (top left) and least significant bit (bottom right).</p>
      </div>
      <div class="values">
        <div class="hex-row">
          <label for="hex-input">Bytes</label>
          <input id="hex-input" spellcheck="false" autocomplete="off" aria-describedby="hex-error">
          <button type="button" class="button" id="random-frame">Random</button>
          <button type="button" class="button" id="clear-frame">Clear</button>
        </div>
        <p id="hex-error" class="hex-error" hidden></p>
        <div id="decoded"></div>
      </div>
    </div>`;
  refreshFrame(false);
}

function activeSignals(msg, result) {
  const active = new Set();
  if (result.ok) {
    for (const s of result.signals) {
      const index = msg.signals.findIndex((x) => x.name === s.name);
      if (index >= 0) active.add(index);
    }
  } else {
    msg.signals.forEach((s, i) => {
      if (typeof s.mux !== 'number') active.add(i);
    });
  }
  return active;
}

function pagesHtml(msg, result) {
  if (!msg.multiplexer) return '';
  const muxSig = msg.signals.find((s) => s.mux === 'switch');
  if (!muxSig) return '';
  const values = [...new Set(msg.signals.filter((s) => typeof s.mux === 'number').map((s) => s.mux))].sort((a, b) => a - b);
  const current = result.ok && result.mux !== null ? Number(result.mux) : null;
  const buttons = values
    .map((v) => {
      const choice = muxSig.choices.find((c) => Number(c.value) === v);
      const label = choice ? `${v}: ${choice.label}` : String(v);
      return `<button type="button" data-page="${v}" aria-pressed="${v === current}">${escapeHtml(label)}</button>`;
    })
    .join('');
  return `<div class="pages" role="group" aria-label="${escapeHtml(muxSig.name)}"><span>${escapeHtml(muxSig.name)}</span>${buttons}</div>`;
}

function matrixHtml(msg, bytes, active) {
  const owner = new Map();
  msg.signals.forEach((sig, si) => {
    if (!active.has(si)) return;
    sig.bits.forEach(([byte, bit], rawBit) => {
      if (byte < msg.dlc) owner.set(byte * 8 + bit, { si, rawBit });
    });
  });
  const cells = ['<div></div>'];
  for (let bit = 7; bit >= 0; bit--) cells.push(`<div class="colhead" aria-hidden="true">${bit}</div>`);
  for (let byte = 0; byte < msg.dlc; byte++) {
    cells.push(`<div class="rowhead">Byte ${byte}</div>`);
    for (let bit = 7; bit >= 0; bit--) {
      const k = byte * 8 + bit;
      const on = (bytes[byte] >> bit) & 1;
      const o = owner.get(k);
      let cls = 'bit' + (on ? ' on' : '');
      let style = `--row:${byte}`;
      let sigAttr = '';
      let label = `Byte ${byte}, bit ${bit} is ${on}`;
      if (o) {
        const sig = msg.signals[o.si];
        const c = colorFor(o.si);
        style += `;--c:${c};--tint:${tint(c, 0.2)}`;
        sigAttr = ` data-sig="${o.si}"`;
        if (sig.length > 1 && o.rawBit === sig.length - 1) cls += ' msb';
        if (sig.length > 1 && o.rawBit === 0) cls += ' lsb';
        label += `, bit ${o.rawBit} of ${sig.name}`;
      }
      cells.push(
        `<button type="button" class="${cls}" data-k="${k}"${sigAttr} style="${style}" aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}">${on}</button>`,
      );
    }
  }
  return cells.join('');
}

function tableHtml(msg, result) {
  if (!result.ok) return `<p class="hex-error">${escapeHtml(sentence(result.error.message))}</p>`;
  const rows = result.signals
    .map((s) => {
      const si = msg.signals.findIndex((x) => x.name === s.name);
      const sig = msg.signals[si];
      const unit = s.unit ? ` ${escapeHtml(s.unit)}` : '';
      const label = s.label ? `<span class="label-text">${escapeHtml(s.label)}</span>` : '';
      return (
        `<tr data-sig="${si}"><td><span class="swatch" style="--c:${colorFor(si)}"></span>${escapeHtml(s.name)}</td>` +
        `<td class="num">${escapeHtml(formatPhysical(sig, s.physical))}${unit}</td>` +
        `<td class="num">${escapeHtml(s.raw)}</td><td>${label}</td></tr>`
      );
    })
    .join('');
  return (
    '<div class="table-scroll"><table class="decoded"><thead><tr>' +
    '<th scope="col">Signal</th><th scope="col">Value</th><th scope="col">Raw</th><th scope="col">Meaning</th>' +
    `</tr></thead><tbody>${rows}</tbody></table></div>`
  );
}

function refreshFrame(fromInput) {
  const msg = currentMessage();
  if (!msg) return;
  const bytes = frameFor(msg);
  const result = state.cf.decode(state.src, msg.id_hex, toHex(bytes));
  const active = activeSignals(msg, result);
  $('pages').innerHTML = pagesHtml(msg, result);
  const matrix = $('matrix');
  matrix.classList.toggle('clock', state.animate);
  matrix.innerHTML = matrixHtml(msg, bytes, active);
  $('decoded').innerHTML = tableHtml(msg, result);
  if (!fromInput) {
    $('hex-input').value = toHex(bytes);
    $('hex-input').removeAttribute('aria-invalid');
    $('hex-error').hidden = true;
  }
  state.animate = false;
}

function highlight(si) {
  for (const cell of document.querySelectorAll('#matrix .bit')) {
    cell.classList.toggle('dim', si !== null && cell.dataset.sig !== String(si));
  }
  for (const row of document.querySelectorAll('#decoded tr[data-sig]')) {
    row.classList.toggle('hot', si !== null && row.dataset.sig === String(si));
  }
}

// ---------------------------------------------------------------- problems view

function renderProblems() {
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

// ---------------------------------------------------------------- code view

function setCodeButtons(enabled) {
  $('copy-code').disabled = !enabled;
  $('download-code').disabled = !enabled;
}

function renderCode() {
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

function flash(button, text) {
  const original = button.textContent;
  button.textContent = text;
  setTimeout(() => {
    button.textContent = original;
  }, 1400);
}

// ---------------------------------------------------------------- compare view

async function useExampleRevisions() {
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

// ---------------------------------------------------------------- wiring

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

  $('message-list').addEventListener('click', (e) => {
    const button = e.target.closest('button[data-index]');
    if (!button) return;
    state.selected = Number(button.dataset.index);
    state.animate = true;
    for (const b of document.querySelectorAll('#message-list button')) {
      b.setAttribute('aria-current', String(b === button));
    }
    renderMessage();
  });

  const detail = $('message-detail');
  detail.addEventListener('click', (e) => {
    const msg = currentMessage();
    if (!msg) return;
    const bytes = frameFor(msg);
    const cell = e.target.closest('.bit');
    if (cell) {
      const k = Number(cell.dataset.k);
      bytes[Math.floor(k / 8)] ^= 1 << (k % 8);
      refreshFrame(false);
      const again = document.querySelector(`#matrix .bit[data-k="${k}"]`);
      if (again) again.focus();
      return;
    }
    const page = e.target.closest('[data-page]');
    if (page) {
      const muxSig = msg.signals.find((s) => s.mux === 'switch');
      if (muxSig) writeRaw(bytes, muxSig, Number(page.dataset.page));
      refreshFrame(false);
      return;
    }
    if (e.target.id === 'random-frame') {
      crypto.getRandomValues(bytes);
      refreshFrame(false);
    } else if (e.target.id === 'clear-frame') {
      bytes.fill(0);
      refreshFrame(false);
    }
  });

  detail.addEventListener('input', (e) => {
    if (e.target.id !== 'hex-input') return;
    const msg = currentMessage();
    const parsed = parseHexInput(e.target.value, msg.dlc);
    const error = $('hex-error');
    if (parsed.error) {
      e.target.setAttribute('aria-invalid', 'true');
      error.textContent = parsed.error;
      error.hidden = false;
      return;
    }
    e.target.removeAttribute('aria-invalid');
    error.hidden = true;
    frameFor(msg).set(parsed.bytes);
    refreshFrame(true);
  });

  const onPoint = (e) => {
    const target = e.target.closest('[data-sig]');
    highlight(target ? Number(target.dataset.sig) : null);
  };
  detail.addEventListener('mouseover', onPoint);
  detail.addEventListener('focusin', onPoint);
  detail.addEventListener('mouseleave', () => highlight(null));

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
