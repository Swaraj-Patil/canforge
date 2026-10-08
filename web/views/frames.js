// Frames view: the message list, the bit grid of the chosen message, and
// its decoded values.
import { $, state } from '../state.js';
import { colorFor, escapeHtml, formatPhysical, parseHexInput, plural, sentence, tint, toHex } from '../format.js';

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

export function currentMessage() {
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

// Packs a raw value into the frame in JavaScript, which hard rule 3 forbids.
// This is its one temporary exception; see the call site and CLAUDE.md.
function writeRaw(bytes, sig, value) {
  const v = BigInt.asUintN(Math.max(sig.length, 1), BigInt(value));
  sig.bits.forEach(([byte, bit], rawBit) => {
    if (byte >= bytes.length) return;
    if ((v >> BigInt(rawBit)) & 1n) bytes[byte] |= 1 << bit;
    else bytes[byte] &= ~(1 << bit) & 0xff;
  });
}

function duration(ms) {
  return ms < 1 ? 'under 1 ms' : `${Math.round(ms)} ms`;
}

export function renderSummary() {
  const s = state.analysis.summary;
  // Non-breaking spaces keep "parsed in 3 ms" on one line in the narrow rail.
  const parsed = `parsed in ${duration(state.loadMs)}`.replace(/ /g, ' ');
  $('db-summary').innerHTML =
    `<h2>${escapeHtml(state.fileName)}</h2>` +
    `<p>${plural(s.messages, 'message')}, ${plural(s.signals, 'signal')}, ${plural(s.nodes, 'node')}, ${parsed}</p>`;
  const total = s.errors + s.warnings + s.infos;
  $('problem-count').textContent = total ? String(total) : '';
}

// The frame as one strip, bit 7 of byte 0 at the left, with one path per
// signal covering its runs of neighbouring bits. Paths rather than a rect
// per bit keep a list of 500 messages light enough to filter while typing.
function barcode(msg) {
  const width = Math.max(msg.dlc * 8, 1);
  const paths = [];
  msg.signals.forEach((sig, si) => {
    const xs = sig.bits.filter(([byte]) => byte < msg.dlc).map(([byte, bit]) => byte * 8 + (7 - bit));
    if (!xs.length) return;
    xs.sort((a, b) => a - b);
    let d = '';
    let start = xs[0];
    for (let i = 1; i <= xs.length; i += 1) {
      if (i < xs.length && xs[i] === xs[i - 1] + 1) continue;
      d += `M${start} 0h${xs[i - 1] - start + 1}v1H${start}z`;
      start = xs[i];
    }
    paths.push(`<path d="${d}" fill="${colorFor(si)}"/>`);
  });
  return `<svg class="barcode" viewBox="0 0 ${width} 1" preserveAspectRatio="none" aria-hidden="true">${paths.join('')}</svg>`;
}

export function renderMessageList() {
  const messages = state.analysis.messages;
  $('message-list').innerHTML = messages
    .map(
      (msg, i) => `<li><button type="button" data-index="${i}" aria-current="${i === state.selected}">
        <span class="name">${escapeHtml(msg.name)}</span>
        <span class="meta"><span class="id">${escapeHtml(msg.id_hex)}</span>, ${plural(msg.dlc, 'byte')}</span>
        <span class="found" hidden></span>
        ${barcode(msg)}</button></li>`,
    )
    .join('');
}

/** Show message `index` of the open file in the frame view. */
export function selectMessage(index) {
  state.selected = index;
  state.animate = true;
  for (const b of document.querySelectorAll('#message-list button')) {
    b.setAttribute('aria-current', String(Number(b.dataset.index) === index));
  }
  renderMessage();
}

export function renderMessage() {
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
  const result = state.cf.decodeLoaded(msg.id_hex, toHex(bytes));
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

export function bindFrames() {
  $('message-list').addEventListener('click', (e) => {
    const button = e.target.closest('button[data-index]');
    if (button) selectMessage(Number(button.dataset.index));
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
      // The one place JavaScript packs bits, a temporary exception to hard
      // rule 3 in CLAUDE.md: choosing a page writes the multiplexer's raw
      // value here. Phase 3 replaces this with cf_encode_loaded. Do not copy
      // the pattern, and do not swap it for a throwaway export.
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
}
