# canforge: guide for Claude Code

canforge is a zero-dependency Rust toolchain for CAN databases (DBC files). It parses, lints, lays out, decodes, diffs, and generates C and Python from them. The same crate builds a command-line tool and a WebAssembly module that powers the browser demo in `web/`, which CI deploys to https://swaraj-patil.github.io/canforge/.

The current plan for the website is in `docs/web-roadmap.md`.

## How correctness works here

Read this before changing anything that turns bits into values or values into bits.

- `reference/canforge_ref.py` is an independent Python reference model. It is the source of truth for parsing, bit layout, lint, decode, diff and code generation.
- `tests/golden/` is generated from it by `python3 reference/make_golden.py`. `tests/golden.rs` requires the Rust output to match byte for byte, with decoded values compared as exact IEEE 754 bit patterns.
- Never edit files in `tests/golden/` by hand. To change behaviour: change the reference model and its tests, regenerate the golden files, then change the Rust until `cargo test` passes.
- The generated C is compiled with `-Werror` under 13 warning flags and differentially tested against the reference by `reference/harness.py` under AddressSanitizer and UBSan (see `.github/workflows/ci.yml`). Keep that green after any change to `src/codegen_c.rs`.

## Hard rules

1. The Rust crate stays dependency-free: `[dependencies]` in `Cargo.toml` stays empty.
2. `web/` stays build-step-free: plain HTML, CSS and ES modules. No bundler, framework or npm package is shipped to the site. Dev-only tooling (Playwright in `web-tests/`) is fine.
3. JavaScript never reimplements bit packing, decoding or encoding. All of it goes through the WebAssembly exports, so the browser runs the same verified code as the CLI.
4. Files people open never leave their browser. No network requests except same-origin assets and Google Fonts. No analytics.
5. Existing WebAssembly exports (`cf_version`, `cf_analyze`, `cf_decode`, `cf_generate`, `cf_diff`) keep working. Add new exports beside them.
6. The site assembly (`scripts/assemble-site.sh`, which the CI `wasm` job and `scripts/dev-site.sh` both run) must include every file the page loads. Copy all of `web/` plus the example files, not a hand-picked list.
7. Interface copy: sentence case, active voice, plain words, no em-dashes, no all-caps labels.

## Commands (local Mac shell, from the repo root)

```sh
cargo test
(cd reference && python3 -m unittest -v test_reference)
python3 reference/make_golden.py && git diff --exit-code tests/golden
cargo build --release --lib --target wasm32-unknown-unknown
node tests/wasm_smoke.mjs target/wasm32-unknown-unknown/release/canforge.wasm
python3 reference/make_large_dbc.py build/large.dbc && node tests/wasm_perf.mjs target/wasm32-unknown-unknown/release/canforge.wasm build/large.dbc
scripts/assemble-site.sh && (cd web-tests && npx playwright test)
```

- The browser tests need a one-time setup: `(cd web-tests && npm ci && npx playwright install chromium)`. They run against `site/`, so assemble it after every change to `web/` or the Rust code.
- `scripts/dev-site.sh` builds the module, assembles `site/` as CI does, and serves it at http://127.0.0.1:8000/.
- The timing tests hard-fail when a budget is missed. `PERF_BUDGETS=report` prints the miss instead; use it only in CI, and only if its timing proves noisy.

## Definition of done

- Every command above passes, including the browser tests and their timing budgets.
- New behaviour is tested at the right level: a reference test and golden file for anything semantic, a Rust unit test for parsing and formatting, a Playwright test for user flows.
- `README.md` describes any user-visible change.
- One commit per feature, with a message that says what changed and why.
