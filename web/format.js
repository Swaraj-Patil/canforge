// Formatting helpers for text, numbers, frame bytes and signal colors. They
// touch neither the page nor its state, so every view can share them.

// Signal colors borrowed from wiring-harness insulation.
const PALETTE = ['#D9692B', '#2E8B57', '#3567C8', '#C79A10', '#7B55C9', '#C23F37', '#1C8A8A', '#8B5E3C', '#C24D93', '#5E7A1F', '#2C4A9A', '#6E7C87'];

export function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
}

export function sentence(text) {
  const t = String(text);
  return t.charAt(0).toUpperCase() + t.slice(1) + (/[.!?]$/.test(t) ? '' : '.');
}

export function plural(n, word) {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

export function colorFor(index) {
  return PALETTE[index % PALETTE.length];
}

export function tint(hex, alpha) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

export function toHex(bytes) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join(' ');
}

export function parseHexInput(text, length) {
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

export function formatPhysical(sig, value) {
  if (value === null) return 'not a number';
  if (sig.value_type !== 'integer') return String(Number(value.toPrecision(7)));
  const d = Math.min(12, Math.max(decimalsOf(sig.factor), decimalsOf(sig.offset)));
  return value.toFixed(d);
}

export function prefixFrom(fileName) {
  const stem = fileName.replace(/\.[^.]*$/, '');
  const snake = stem
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/[^A-Za-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .toLowerCase();
  if (!snake) return 'canbus';
  return /^[0-9]/.test(snake) ? `x_${snake}` : snake;
}
