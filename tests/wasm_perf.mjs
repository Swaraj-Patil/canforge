// Times the real canforge.wasm in Node on a large database and checks the
// budgets in docs/web-roadmap.md. Every measurement is printed on every run,
// so a regression shows in the CI log even when it stays inside its budget.
//
// Usage:
//   python3 reference/make_large_dbc.py build/large.dbc
//   node tests/wasm_perf.mjs <path to canforge.wasm> build/large.dbc
//
// A missed budget fails the run. With PERF_BUDGETS=report it is printed
// instead, for machines whose timing is too noisy to gate on.
import { readFileSync } from 'node:fs';
import { loadCanforge } from '../web/canforge.js';

const [wasmPath, dbcPath] = process.argv.slice(2);
if (!wasmPath || !dbcPath) {
  console.error('usage: node tests/wasm_perf.mjs <path to canforge.wasm> <large.dbc>');
  process.exit(2);
}
const reportOnly = process.env.PERF_BUDGETS === 'report';

const cf = await loadCanforge(readFileSync(wasmPath));
const src = readFileSync(dbcPath, 'utf8');

function timed(fn) {
  const start = performance.now();
  const result = fn();
  return [performance.now() - start, result];
}

function median(times) {
  const sorted = [...times].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

const ms = (t) => (t < 0.1 ? `${(t * 1000).toFixed(1)} µs` : `${t.toFixed(t < 10 ? 2 : 1)} ms`);
let misses = 0;

function report(what, time, budget, note = '') {
  const verdict = budget === undefined ? '' : time <= budget ? `  budget ${budget} ms` : `  OVER the ${budget} ms budget`;
  console.log(`${what.padEnd(56)} ${ms(time).padStart(10)}${verdict}${note ? `  (${note})` : ''}`);
  if (budget !== undefined && time > budget) misses += 1;
}

// Frames to decode: every message with deterministic pseudo-random bytes.
let seed = 0x2545f491;
function nextByte() {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return seed & 0xff;
}
function hexOf(length) {
  return Array.from({ length }, () => nextByte().toString(16).padStart(2, '0')).join('');
}

// The first load pays for growing WebAssembly memory, so it is reported apart
// from the warm median. The budget applies to both.
const [cold, analysis] = timed(() => cf.load(src));
if (!analysis.ok) {
  console.error(`wasm perf: ${dbcPath} did not load: ${analysis.error.message} (line ${analysis.error.line})`);
  process.exit(1);
}
const s = analysis.summary;
console.log(`${dbcPath}: ${s.messages} messages, ${s.signals} signals, ${(src.length / 1024).toFixed(0)} KB`);
const warm = [];
for (let i = 0; i < 10; i += 1) warm.push(timed(() => cf.load(src))[0]);
report('load, first call (cf_load)', cold, 500);
report('load, median of 10 (cf_load)', median(warm), 500);

const frames = analysis.messages.map((m) => [m.id_hex, hexOf(m.dlc)]);
function decodeOrExit(fn, id, hex) {
  const d = fn(id, hex);
  if (!d.ok) {
    console.error(`wasm perf: decoding ${id} ${hex} failed: ${d.error.message}`);
    process.exit(1);
  }
}

// Decoding through the loaded database: the mean over many frames, since one
// frame takes too little time to measure on its own.
const count = 10000;
const [loadedTotal] = timed(() => {
  for (let i = 0; i < count; i += 1) {
    const [id, hex] = frames[(i * 37) % frames.length];
    decodeOrExit(cf.decodeLoaded, id, hex);
  }
});
const perLoaded = loadedTotal / count;
report(`decode one frame, mean of ${count.toLocaleString('en-US')} (cf_decode_loaded)`, perLoaded, undefined,
  `100,000 frames would take ${((perLoaded * 100000) / 1000).toFixed(1)} s`);

// The old path re-parses the whole file for every frame.
const reparsed = [];
for (let i = 0; i < 50; i += 1) {
  const [id, hex] = frames[(i * 37) % frames.length];
  reparsed.push(timed(() => decodeOrExit((x, y) => cf.decode(src, x, y), id, hex))[0]);
}
const perReparse = median(reparsed);
report('decode one frame, median of 50 (cf_decode)', perReparse, undefined,
  `re-parses the file; 100,000 frames would take ${((perReparse * 100000) / 1000).toFixed(0)} s`);

const [genTime, gen] = timed(() => cf.generateLoaded('c', 'large', 'large.dbc'));
if (!gen.ok) {
  console.error(`wasm perf: generating C failed: ${gen.error.message}`);
  process.exit(1);
}
const lines = gen.files.reduce((n, f) => n + f.content.split('\n').length, 0);
report('generate C (cf_generate_loaded)', genTime, undefined, `${lines.toLocaleString('en-US')} lines`);

// One signal's C, as the inspector asks for it. Each call lints and names the
// whole database first, so it refuses exactly when full generation would.
const snippetTimes = [];
for (let i = 0; i < 20; i += 1) {
  const mi = (i * 37) % analysis.messages.length;
  const [t, snip] = timed(() => cf.signalCodeLoaded(mi, 0, 'large'));
  if (!snip.ok) {
    console.error(`wasm perf: the C for message ${mi} failed: ${snip.error.message}`);
    process.exit(1);
  }
  snippetTimes.push(t);
}
report('C for one signal, median of 20 (cf_signal_code_loaded)', median(snippetTimes));

if (misses && !reportOnly) {
  console.error(`wasm perf: ${misses} budget(s) missed`);
  process.exit(1);
}
console.log(misses ? `wasm perf: ${misses} budget(s) missed (PERF_BUDGETS=report, not failing)` : 'wasm perf: all budgets met');
