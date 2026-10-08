// Runs the real canforge.wasm in Node and checks it against the golden files,
// so the browser build is held to the same standard as the CLI.
// Usage: node tests/wasm_smoke.mjs site/canforge.wasm
import { readFileSync } from 'node:fs';
import { loadCanforge } from '../web/canforge.js';

const wasmPath = process.argv[2];
if (!wasmPath) {
  console.error('usage: node tests/wasm_smoke.mjs <path to canforge.wasm>');
  process.exit(2);
}

let failures = 0;
function check(ok, what) {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) failures += 1;
}

const read = (path) => readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
const cf = await loadCanforge(readFileSync(wasmPath));
const dbc = read('examples/powertrain.dbc');

check(typeof cf.version() === 'string', 'reports its version');

const a = cf.analyze(dbc);
check(a.ok && a.messages.length === 7 && a.summary.signals === 40, 'reads 7 messages and 40 signals');
check(a.ok && a.diagnostics.length === 1 && a.diagnostics[0].rule === 'I002', 'lint reports only the CAN FD note');

const c = cf.generate(dbc, 'c', 'powertrain', 'powertrain.dbc');
check(c.ok && c.files[0].content === read('tests/golden/powertrain.h'), 'generated header matches the golden file');
check(c.ok && c.files[1].content === read('tests/golden/powertrain.c'), 'generated source matches the golden file');
const py = cf.generate(dbc, 'python', 'powertrain', 'powertrain.dbc');
check(py.ok && py.files[0].content === read('tests/golden/powertrain.py'), 'generated Python matches the golden file');

const d = cf.decode(dbc, '0x100', 'e8 03 03 5a 18 fc 00 05');
const find = (name) => (d.ok ? d.signals.find((s) => s.name === name) : null);
check(find('VehicleSpeed') && Math.abs(find('VehicleSpeed').physical - 10) < 1e-9, 'decodes VehicleSpeed as 10 km/h');
check(find('GearPosition') && find('GearPosition').label === 'Drive', 'decodes GearPosition as Drive');

const diff = cf.diff(read('tests/fixtures/diff/v1.dbc'), read('tests/fixtures/diff/v2.dbc'));
check(diff.ok && diff.verdict === 'breaking' && diff.changes.length === 13, 'diff finds 13 changes and a breaking verdict');

const bad = cf.analyze('BO_ 1 M 8 A\n');
check(!bad.ok && bad.error.line === 1, 'reports parse errors with a line number');

for (let i = 0; i < 2000; i += 1) {
  cf.decode(dbc, '0x300', `0${i % 3} 00 00 00 00 00 00 00`);
}
check(cf.analyze(dbc).ok, 'still healthy after 2000 calls (no leaks or corruption)');

// The loaded entry points: cf_load parses once and keeps the database.
const fresh = await loadCanforge(readFileSync(wasmPath));
const before = fresh.decodeLoaded('0x100', 'e8 03 03 5a 18 fc 00 05');
check(!before.ok && before.error.message.includes('cf_load'), 'loaded calls before cf_load say to call it first');

check(JSON.stringify(cf.load(dbc)) === JSON.stringify(a), 'cf_load returns the same analysis as cf_analyze');

// Every frame of the golden decode vectors through cf_decode_loaded. Physical
// values are compared as exact IEEE 754 bit patterns of the parsed JSON
// numbers, so this holds the JSON path to the reference model too.
const bitsOf = (x) => {
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, x);
  return view.getBigUint64(0).toString(16).padStart(16, '0');
};
// JSON has no NaN or infinity, so those arrive as null.
const nonFinite = (bits) => ((BigInt(`0x${bits}`) >> 52n) & 0x7ffn) === 0x7ffn;
const clean = (text) => text.replace(/[\t\r\n]/g, ' ');
const vectors = new Map();
for (const line of read('tests/golden/decode_vectors.tsv').split('\n')) {
  if (!line || line.startsWith('#')) continue;
  const row = line.split('\t');
  if (!vectors.has(row[0])) vectors.set(row[0], []);
  vectors.get(row[0]).push(row);
}
const mismatches = [];
let values = 0;
for (const [index, rows] of vectors) {
  const [, frameId, extended, data, mux] = rows[0];
  const d = cf.decodeLoaded(`0x${frameId}`, data);
  const wrong = [];
  if (!d.ok) {
    wrong.push(d.error.message);
  } else {
    if (d.extended !== (extended === '1')) wrong.push('decoded as the wrong frame format');
    if ((d.mux ?? '-') !== mux) wrong.push(`multiplexer ${d.mux}, reference ${mux}`);
    const names = d.signals.map((s) => s.name).join(',');
    if (names !== rows.map((r) => r[5]).join(',')) {
      wrong.push(`signals ${names}`);
    } else {
      d.signals.forEach((s, i) => {
        const [, , , , , name, raw, physicalBits, label] = rows[i];
        if (s.raw !== raw) wrong.push(`${name} raw ${s.raw}, reference ${raw}`);
        const same = s.physical === null ? nonFinite(physicalBits) : bitsOf(s.physical) === physicalBits;
        if (!same) wrong.push(`${name} physical ${s.physical}, reference bits ${physicalBits}`);
        if (clean(s.label ?? '-') !== label) wrong.push(`${name} label ${s.label}, reference ${label}`);
        values += 1;
      });
    }
  }
  if (wrong.length) mismatches.push(`frame ${index} (${frameId} ${data}): ${wrong.join('; ')}`);
}
check(
  vectors.size === 280 && values > 1000 && mismatches.length === 0,
  `cf_decode_loaded matches all ${vectors.size} golden frames bit for bit (${values} values)`,
);
for (const m of mismatches.slice(0, 5)) console.log(`     ${m}`);

const cl = cf.generateLoaded('c', 'powertrain', 'powertrain.dbc');
check(
  cl.ok && cl.files[0].content === read('tests/golden/powertrain.h') && cl.files[1].content === read('tests/golden/powertrain.c'),
  'cf_generate_loaded C matches the golden files',
);
const pl = cf.generateLoaded('python', 'powertrain', 'powertrain.dbc');
check(pl.ok && pl.files[0].content === read('tests/golden/powertrain.py'), 'cf_generate_loaded Python matches the golden file');

// What each signal can carry, from the analysis JSON, against the reference.
const loadedAnalysis = cf.load(dbc);
const rangeRows = read('tests/golden/ranges.tsv')
  .split('\n')
  .filter((l) => l && !l.startsWith('#'))
  .map((l) => l.split('\t'));
const rangeMismatches = [];
let rangeIndex = 0;
for (const m of loadedAnalysis.messages) {
  for (const s of m.signals) {
    const [msgName, sigName, rawMin, rawMax, minBits, maxBits] = rangeRows[rangeIndex] || [];
    rangeIndex += 1;
    const sameBits = (x, bits) => (x === null ? nonFinite(bits) : bitsOf(x) === bits);
    const ok =
      msgName === m.name &&
      sigName === s.name &&
      s.raw_min === rawMin &&
      s.raw_max === rawMax &&
      sameBits(s.physical_min, minBits) &&
      sameBits(s.physical_max, maxBits);
    if (!ok) rangeMismatches.push(`${m.name}.${s.name}: ${s.raw_min} ${s.raw_max} ${s.physical_min} ${s.physical_max}`);
  }
}
check(
  rangeIndex === 40 && rangeRows.length === 40 && rangeMismatches.length === 0,
  `the analysis gives every signal's representable range bit for bit (${rangeIndex} signals)`,
);
for (const m of rangeMismatches.slice(0, 5)) console.log(`     ${m}`);

// The generated C of every signal, each part found verbatim in the golden files.
const goldenH = read('tests/golden/powertrain.h');
const goldenC = read('tests/golden/powertrain.c');
let snippets = 0;
const snippetMisses = [];
loadedAnalysis.messages.forEach((m, mi) => {
  m.signals.forEach((s, si) => {
    const snip = cf.signalCodeLoaded(mi, si, 'powertrain');
    const found =
      snip.ok &&
      goldenH.includes(snip.field) &&
      [snip.pack, snip.unpack, snip.functions].every((part) => part && goldenC.includes(part));
    if (found) snippets += 1;
    else snippetMisses.push(`${m.name}.${s.name}`);
  });
});
check(snippets === 40, `cf_signal_code_loaded gives C found verbatim in the golden files for all ${snippets} signals`);
for (const m of snippetMisses.slice(0, 5)) console.log(`     ${m}`);
const badIndex = cf.signalCodeLoaded(0, 99, 'powertrain');
check(!badIndex.ok && badIndex.error.message.includes('has no signal 99'), 'cf_signal_code_loaded reports a signal that does not exist');

const failed = cf.load('BO_ 1 M 8 A\n');
const still = cf.decodeLoaded('0x100', 'e8 03 03 5a 18 fc 00 05');
check(
  !failed.ok && failed.error.line === 1 && still.ok && still.message === 'VehicleStatus',
  'a file that does not parse leaves the previous database loaded',
);

for (let i = 0; i < 2000; i += 1) {
  cf.decodeLoaded('0x300', `0${i % 3} 00 00 00 00 00 00 00`);
}
check(cf.decodeLoaded('0x600', '20 b2 09 20 82 0a 96 c0').ok, 'still healthy after 2000 loaded decodes');

if (failures) {
  console.error(`wasm smoke test failed: ${failures} check(s)`);
  process.exit(1);
}
console.log('wasm smoke test passed');
