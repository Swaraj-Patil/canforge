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

const ms = (t) => `${t.toFixed(t < 10 ? 2 : 1)} ms`;
let misses = 0;

function report(what, time, budget, note = '') {
  const verdict = budget === undefined ? '' : time <= budget ? `  budget ${budget} ms` : `  OVER the ${budget} ms budget`;
  console.log(`${what.padEnd(44)} ${ms(time).padStart(10)}${verdict}${note ? `  (${note})` : ''}`);
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
const [cold, analysis] = timed(() => cf.analyze(src));
if (!analysis.ok) {
  console.error(`wasm perf: ${dbcPath} did not load: ${analysis.error.message} (line ${analysis.error.line})`);
  process.exit(1);
}
const s = analysis.summary;
console.log(`${dbcPath}: ${s.messages} messages, ${s.signals} signals, ${(src.length / 1024).toFixed(0)} KB`);
const warm = [];
for (let i = 0; i < 10; i += 1) warm.push(timed(() => cf.analyze(src))[0]);
report('load, first call (cf_analyze)', cold, 500);
report('load, median of 10 (cf_analyze)', median(warm), 500);

const frames = analysis.messages.map((m) => [m.id_hex, hexOf(m.dlc)]);
const decodes = [];
for (let i = 0; i < 50; i += 1) {
  const [id, hex] = frames[(i * 37) % frames.length];
  const [t, d] = timed(() => cf.decode(src, id, hex));
  if (!d.ok) {
    console.error(`wasm perf: decoding ${id} ${hex} failed: ${d.error.message}`);
    process.exit(1);
  }
  decodes.push(t);
}
const perFrame = median(decodes);
report('decode one frame, median of 50 (cf_decode)', perFrame, undefined,
  `re-parses the file; 100,000 frames would take ${((perFrame * 100000) / 1000).toFixed(0)} s`);

const [genTime, gen] = timed(() => cf.generate(src, 'c', 'large', 'large.dbc'));
if (!gen.ok) {
  console.error(`wasm perf: generating C failed: ${gen.error.message}`);
  process.exit(1);
}
const lines = gen.files.reduce((n, f) => n + f.content.split('\n').length, 0);
report('generate C (cf_generate)', genTime, undefined, `${lines.toLocaleString('en-US')} lines`);

if (misses && !reportOnly) {
  console.error(`wasm perf: ${misses} budget(s) missed`);
  process.exit(1);
}
console.log(misses ? `wasm perf: ${misses} budget(s) missed (PERF_BUDGETS=report, not failing)` : 'wasm perf: all budgets met');
