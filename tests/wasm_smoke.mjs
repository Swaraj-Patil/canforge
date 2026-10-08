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

if (failures) {
  console.error(`wasm smoke test failed: ${failures} check(s)`);
  process.exit(1);
}
console.log('wasm smoke test passed');
