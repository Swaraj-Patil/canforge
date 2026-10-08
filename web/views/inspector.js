// Signal inspector: what the file says about one signal, what its bits can
// carry, and the C that canforge generates for it. Every fact comes from the
// WebAssembly module (bit positions, ranges, lint findings and code); this
// file only puts them into words.
import { $, codePrefix, currentMessage, state } from '../state.js';
import { colorFor, escapeHtml, formatPhysical, sentence } from '../format.js';

const MINUS = '−';

// A number as the file writes it.
function num(x) {
  return x === null ? 'unbounded' : String(x).replace(/^-/, MINUS);
}

// A computed bound at the precision of the signal's scaling, so 3276.7 and
// not 3276.7000000000003. Whole numbers keep every digit: 2^64 reads
// 18446744073709551616.
function bound(sig, x) {
  if (x === null) return 'unbounded';
  let text = formatPhysical(sig, x);
  if (text.includes('.') && !text.includes('e')) text = text.replace(/\.?0+$/, '');
  return text.replace(/^-/, MINUS);
}

function layoutText(sig) {
  const order = sig.byte_order === 'motorola' ? 'Motorola' : 'Intel';
  if (sig.length === 1) return `${order}, 1 bit, at bit ${sig.start}`;
  const first = sig.byte_order === 'motorola' ? 'most' : 'least';
  return `${order}, ${sig.length} bits, starting at bit ${sig.start}, ${first} significant bit first`;
}

// "Byte 0 bits 7 to 0, byte 1 bits 7 to 4": the bits of each byte the
// signal touches, which are always one unbroken run.
function bitsText(sig) {
  const bytes = new Map();
  for (const [byte, bit] of sig.bits) {
    const [hi, lo] = bytes.get(byte) || [bit, bit];
    bytes.set(byte, [Math.max(hi, bit), Math.min(lo, bit)]);
  }
  return [...bytes]
    .sort((a, b) => a[0] - b[0])
    .map(([byte, [hi, lo]], i) => `${i ? 'byte' : 'Byte'} ${byte} ${hi === lo ? `bit ${hi}` : `bits ${hi} to ${lo}`}`)
    .join(', ');
}

function kindText(sig) {
  if (sig.value_type === 'float32') return 'IEEE 754 single-precision float';
  if (sig.value_type === 'float64') return 'IEEE 754 double-precision float';
  return sig.signed ? "Signed integer (two's complement)" : 'Unsigned integer';
}

function muxText(msg, sig) {
  if (sig.mux === 'switch') return 'This is the multiplexer: its value selects which signals the frame carries.';
  if (typeof sig.mux !== 'number') return '';
  const muxSig = msg.signals.find((s) => s.mux === 'switch');
  if (!muxSig) return `Present when the multiplexer is ${sig.mux}.`;
  const choice = muxSig.choices.find((c) => Number(c.value) === sig.mux);
  return `Present when ${muxSig.name} is ${sig.mux}${choice ? ` (${choice.label})` : ''}.`;
}

// Leaves out a factor of 1 and an offset of 0, as the generated C does.
function scalingText(sig) {
  let text = `physical = ${sig.value_type === 'integer' ? 'raw' : 'the float'}`;
  if (sig.factor !== 1) text += ` × ${num(sig.factor)}`;
  if (sig.offset > 0) text += ` + ${num(sig.offset)}`;
  if (sig.offset < 0) text += ` ${MINUS} ${num(-sig.offset)}`;
  return sig.factor === 1 && sig.offset === 0 ? `${text} (no scaling)` : text;
}

function withUnit(text, sig) {
  return sig.unit ? `${text} ${sig.unit}` : text;
}

function declaredText(sig) {
  if (sig.minimum === 0 && sig.maximum === 0) return 'Not specified (the file gives [0|0])';
  return withUnit(`${num(sig.minimum)} to ${num(sig.maximum)}`, sig);
}

function representableText(sig) {
  if (sig.raw_min === null) return 'None: the length is not valid';
  const physical = withUnit(`${bound(sig, sig.physical_min)} to ${bound(sig, sig.physical_max)}`, sig);
  if (sig.value_type !== 'integer') return `${physical} (any finite ${sig.value_type} value)`;
  return `${physical} (raw ${sig.raw_min.replace(/^-/, MINUS)} to ${sig.raw_max})`;
}

function row(term, html) {
  return html ? `<dt>${term}</dt><dd>${html}</dd>` : '';
}

function choicesHtml(sig) {
  if (!sig.choices.length) return '';
  const rows = sig.choices
    .map((c) => `<tr><td>${escapeHtml(c.value.replace(/^-/, MINUS))}</td><td>${escapeHtml(c.label)}</td></tr>`)
    .join('');
  return `<table class="choices"><tbody>${rows}</tbody></table>`;
}

function problemsHtml(msg, sig) {
  const names = { error: 'Error', warning: 'Warning', info: 'Note' };
  const found = state.analysis.diagnostics.filter((d) => d.message_name === msg.name && d.signal_name === sig.name);
  return found
    .map((d) => `<p class="finding ${escapeHtml(d.severity)}"><span class="sev">${names[d.severity] || ''}</span> ${escapeHtml(d.rule)}: ${escapeHtml(sentence(d.message))}</p>`)
    .join('');
}

function codeHtml(si) {
  const code = state.cf.signalCodeLoaded(state.selected, si, codePrefix());
  if (!code.ok) {
    const why = state.analysis.summary.errors
      ? 'Code generation is off while the file has errors. The Problems tab lists them.'
      : sentence(code.error.message);
    return `<p class="muted">${escapeHtml(why)}</p>`;
  }
  const block = (caption, text) =>
    `<figure><figcaption>${caption}</figcaption><pre class="code" tabindex="0">${escapeHtml(text)}</pre></figure>`;
  return (
    `<p class="muted">The same text canforge writes to ${escapeHtml(code.header)} and ${escapeHtml(code.source)}.</p>` +
    block(`Struct member in ${escapeHtml(code.header)}`, code.field) +
    block(`In <code>${escapeHtml(code.pack_function)}()</code>`, code.pack) +
    block(`In <code>${escapeHtml(code.unpack_function)}()</code>`, code.unpack) +
    block(`Decode, encode and range check in ${escapeHtml(code.source)}`, code.functions)
  );
}

/** Show the inspected signal of the selected message, or hide the panel. */
export function renderInspector() {
  const panel = $('inspector');
  if (!panel) return;
  const msg = currentMessage();
  const si = state.inspect;
  const sig = msg && si !== null ? msg.signals[si] : null;
  panel.hidden = !sig;
  if (!sig) {
    panel.innerHTML = '';
    return;
  }
  const receivers = sig.receivers.filter((r) => r !== 'Vector__XXX');
  const layout = sig.bits.length
    ? `${escapeHtml(layoutText(sig))}<span class="sub">${escapeHtml(bitsText(sig))}. In the file: <code>${sig.start}|${sig.length}@${sig.byte_order === 'motorola' ? 0 : 1}${sig.signed ? '-' : '+'}</code></span>`
    : 'Not valid: a signal must be 1 to 64 bits long';
  panel.innerHTML = `
    <div class="inspector-head">
      <h3 id="inspector-title"><span class="swatch" style="--c:${colorFor(si)}"></span>${escapeHtml(sig.name)}</h3>
      <button type="button" class="button small" data-close-inspector>Close</button>
    </div>
    ${sig.comment ? `<p class="comment">${escapeHtml(sig.comment)}</p>` : ''}
    <dl>
      ${row('Layout', layout)}
      ${row('Type', escapeHtml(kindText(sig)))}
      ${row('Multiplexing', escapeHtml(muxText(msg, sig)))}
      ${row('Scaling', escapeHtml(scalingText(sig)))}
      ${row('Declared range', escapeHtml(declaredText(sig)))}
      ${row('Range the bits can hold', escapeHtml(representableText(sig)))}
      ${row('Unit', escapeHtml(sig.unit || 'None'))}
      ${row('Receivers', escapeHtml(receivers.join(', ') || 'None'))}
      ${row('Value descriptions', choicesHtml(sig))}
      ${row('Problems', problemsHtml(msg, sig))}
    </dl>
    <h4>Generated C</h4>
    ${codeHtml(si)}`;
}
