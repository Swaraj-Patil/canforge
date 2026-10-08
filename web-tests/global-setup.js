// Runs once before the browser tests: checks that site/ has been assembled,
// and writes the large synthetic database that the timing tests open.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));

export default function globalSetup() {
  for (const file of ['site/index.html', 'site/canforge.wasm']) {
    if (!existsSync(root + file)) {
      throw new Error(
        `${file} is missing, so there is no site to test. Build and assemble it first:\n` +
          '  cargo build --release --lib --target wasm32-unknown-unknown && scripts/assemble-site.sh',
      );
    }
  }
  execFileSync('python3', ['reference/make_large_dbc.py', 'build/large.dbc'], { cwd: root, stdio: 'inherit' });
}
