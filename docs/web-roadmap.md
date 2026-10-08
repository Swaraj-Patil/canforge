# Website roadmap

The browser demo at https://swaraj-patil.github.io/canforge/ works, but it is a viewer. This plan turns it into a tool an engineer would keep open, and makes it obvious to a non-expert within thirty seconds.

Work through the phases in order. Each phase leaves the site deployable, ends with every check in `CLAUDE.md` passing, and lists its own acceptance checks. Before starting a phase, propose a file-level plan and wait for approval.

## Who it is for

- Embedded and vehicle-software engineers: open a real DBC (often thousands of signals), find a message fast, decode or build frames, read logs, and catch mistakes before they ship.
- Reviewers and recruiters with no CAN background: understand what it does and see it work in under a minute.

## What exists today

| Area | Today |
|---|---|
| Frames | Message list with a bit-layout barcode, clickable bit grid, hex input, decoded values table, multiplexer page buttons |
| Problems | Lint findings with rule, line and message |
| Generated code | C and Python, editable prefix, copy and download |
| Compare | Two revisions, a verdict, grouped changes |
| Files | `web/index.html`, `web/style.css`, `web/app.js` (all view logic), `web/canforge.js` (WebAssembly glue) |

Known limits: every bit click re-parses the whole DBC inside WebAssembly; there is no search; nothing can be edited, encoded or replayed from a log; there are no browser tests.

## Principles for every phase

- Keep the visual system: the paper and ink color tokens, the harness-color signal palette, IBM Plex Sans for text, Plex Sans Condensed for headings, Plex Mono only for real data (hex, bits, code). No generic card grids, no gradients, one orchestrated motion moment, `prefers-reduced-motion` respected, visible keyboard focus everywhere.
- Split `web/app.js` into modules as it grows (for example `web/state.js`, `web/views/frames.js`, `web/views/log.js`). Plain ES modules work on GitHub Pages without a bundler.
- Performance budgets on a typical laptop: a bit flip updates the page within 16 ms on the example bus and within 50 ms on a 5,000-signal database; a 100,000-frame log decodes in under 2 seconds.
- Error messages say what happened and how to fix it.

---

## Phase 1: foundation

### 1.1 Load once, then reuse

Why: re-parsing on every click is fine for the example and slow for real databases. Log replay needs this too.

- Refactor `src/json.rs` so each output has a core that takes `&Database`; the existing functions that take DBC text call it.
- In `src/wasm.rs`, keep the parsed database in `static LOADED: Mutex<Option<Database>>`. Add `cf_load(src)` (returns the same JSON as `cf_analyze`), `cf_decode_loaded(frame_id, hex)` and `cf_generate_loaded(lang, prefix, source_name)`.
- Build a frame-ID index on load instead of scanning every message per frame.
- Show load time in the database summary, for example "412 messages, parsed in 18 ms".
- Add `reference/make_large_dbc.py`: a deterministic synthetic database of about 500 messages and 5,000 signals for performance checks. CI generates it; it is not committed.

### 1.2 Search

- A search field above the message list filters by message name, frame ID (`0x400` or `1024`) or signal name, and says which signal matched. `/` focuses it and Esc clears it.

### 1.3 Byte values and signal names on the grid

- Each grid row shows its byte in hex and decimal at the right edge, updating live.
- Runs of three or more adjacent bits from the same signal show the signal name across the run, on an overlay that does not block clicks.

### 1.4 Signal inspector

- Selecting a signal (its row in the values table) opens an inspector: the layout in words ("Motorola, 12 bits, starting at bit 7, most significant bit first"), scale, offset, declared and representable range, unit, receivers, comment, value descriptions, and the generated C for that signal.
- Rust: `codegen_c::signal_snippet(db, message_index, signal_index, prefix)` reuses `pack_lines`, `unpack_lines` and `signal_functions`, so the snippet is byte-identical to the full file. Export it as `cf_signal_code_loaded(message, signal, prefix)`.

### 1.5 Remember and link

- Remember the last opened file in `localStorage`, only in this browser, for files up to 2 MB. Say so in the interface and offer "Forget this file".
- Keep the view in the URL hash for the example bus, for example `#t=frames&m=WheelSpeeds&b=20b20920820a96c0`, and add a "Copy link" button. Opening the link restores the same tab, message and bytes.

### 1.6 Developer loop and browser tests

- `scripts/dev-site.sh`: build the WebAssembly module, assemble `site/` exactly as CI does, and serve it on port 8000.
- Playwright tests in `web-tests/` (with its own `package.json`, never deployed), run in the CI `wasm` job against the assembled `site/` using Chromium.

### Acceptance

- The smoke test checks that `cf_decode_loaded` matches all 280 frames in `tests/golden/decode_vectors.tsv` bit for bit (read the TSV in Node and compare `Float64` bit patterns).
- A Rust test asserts that every signal snippet appears verbatim in the full generated C for the example.
- The large database loads in under 500 ms in the WebAssembly build. Record the measured numbers in the commit message.
- Browser tests: the example shows 7 messages; flipping byte 0, bit 0 of VehicleStatus changes VehicleSpeed by 0.01 km/h; searching `yaw` finds WheelSpeeds; Problems shows the I002 note; Generated code lists `powertrain.h`; Compare with the example revisions reports breaking changes; a copied link restores the same frame.

---

## Phase 2: edit the DBC in the browser

### 2.1 Source tab

- A new tab shows the DBC text in an editable textarea with a line-number gutter. No editor library.
- The gutter marks lines with findings by severity. Hovering or focusing a mark shows the finding.
- Analysis re-runs 300 ms after typing stops. On a parse error, keep the last good analysis on screen and pin the error to its line.
- "Revert to opened file" and "Download edited file" (saved as `name-edited.dbc`). The database summary shows when the file has unsaved edits.

### 2.2 Link problems, source and frames

- Every finding offers "Show line", which opens Source at that line and highlights it.
- Signal findings also offer "Show in frame", which selects the message and outlines the bits involved in red.
- Add `bits` to `Diag` for E001 (the shared bits) and E002 (the bits past the end of the frame). Mirror it in the reference model, add it as a column in `tests/golden/lint.tsv`, and include it in the JSON.

### Acceptance

- Browser test: in Source, change `GearPosition`'s start bit from 16 to 14. Within a second, Problems shows E001 for `VehicleSpeed` and `GearPosition` at bits 14 and 15, Frames outlines those two bits in red, and "Revert to opened file" restores the clean state.

---

## Phase 3: build frames from values

Why: engineers more often need to build a frame from values ("which bytes mean 88 km/h in Drive?") than to read one.

### 3.1 Semantics, matched exactly to the generated C `encode` functions

For integer signals:

1. `r = (value - offset) / factor`
2. If `!(r >= lo)`, set `r = lo` (this also catches NaN). If `r > hi`, set `r = hi`. `lo` and `hi` are the same bounds the C uses: `f64_at_least(raw_min)` and `f64_at_most(raw_max)`.
3. Round half away from zero: signed `trunc(r >= 0 ? r + 0.5 : r - 0.5)`, unsigned `trunc(r + 0.5)`.

For float32 signals, clamp to plus or minus `FLT_MAX` and store the IEEE 754 bits. Float64 signals store the bits of `r` directly. Golden vectors for float signals use finite inputs only.

Encoding starts from a base frame and overwrites only the assigned signals. If the multiplexer is not assigned, it keeps its value from the base frame. Values assigned to signals outside the active group are reported as ignored. Every clamp is reported with the input, the value used and the reason.

### 3.2 Build order

1. Reference model: `encode_raw(signal, value)` and `encode(db, frame_id, values, base)`, with tests.
2. Golden file `tests/golden/encode_vectors.tsv` covering negatives, out-of-range values, NaN, float signals, multiplexer pages and 64-bit fields.
3. Extend `reference/harness.py` so the C harness checks each generated `*_encode` against the reference `encode_raw` on the same inputs.
4. Rust `src/encode.rs` with a golden test, and `cf_encode_loaded(frame_id, base_hex, assignments)`, where assignments are `name=value` lines so no JSON parser is needed. It returns `{ok, hex, clamped, ignored}`.
5. Interface: values in the table become editable (Enter applies, Esc cancels). Signals with value descriptions get a menu. Clamped values show a one-line note, for example "Clamped to 655.35 km/h, the largest value these 16 bits hold."

### Acceptance

- The golden encode test and the C harness encode check pass.
- Browser test: entering 88 for VehicleSpeed produces bytes starting `60 22`, and the table then reads 88.00 km/h.

---

## Phase 4: replay logs and plot signals

Why: real debugging starts from a recording. Engineers capture traffic with `candump` and need to see signals over time.

### 4.1 Formats (Rust `src/log.rs`, fixtures in `tests/fixtures/logs/`)

- candump log files: `(1609459200.123456) can0 100#E803035A18FC0005`. Eight hex digits mean an extended ID. `##` marks CAN FD (`100##1E803...`, where the digit after `##` is the flags nibble). `R` after `#` marks a remote frame, which is skipped.
- candump terminal output: `can0  100   [8]  E8 03 03 5A 18 FC 00 05`, optionally with a leading `(timestamp)`.
- CSV: `time,id,data` with an optional header, data as hex.
- Lines that cannot be read are counted, and the first 20 are listed with line numbers. They never stop the decode.

### 4.2 Decoding and the sample log

- `cf_decode_log_loaded(text)` returns columnar series per signal: `{signals: [{message, name, unit, t: [], v: []}], frames, unknown_ids, skipped}`. Multiplexed signals only get points when their page is active. Frames without timestamps use their index as time.
- `reference/make_sample_log.py` writes `examples/drive.log`: 30 seconds of the example bus with the car accelerating from 0 to 80 km/h and braking, gear changes, pack voltage sagging under current, inverter pages cycling, and wheel speeds following the vehicle speed with a little noise. Build it with the reference model's bit packing so it is consistent with the DBC. Copy it into `site/examples/` in CI and in the dev script.

### 4.3 Log tab

- Load a log by file picker, drag and drop, or "Use the example drive". Show frame count, duration, per-message counts and unknown IDs.
- Pick signals from a searchable checklist and plot them as stacked line charts sharing one time axis, drawn with our own SVG code. Decimate to a minimum and maximum per pixel column so 100,000+ points stay smooth.
- A shared cursor shows every plotted value at the hovered time. Clicking a time opens that exact frame in the Frames view.
- "Export CSV" downloads the decoded series.

### Acceptance

- Fixture tests cover every format and every skip reason.
- The smoke test decodes `examples/drive.log` and checks the frame count and a known value at a known time.
- A 100,000-frame synthetic log decodes in under 2 seconds.
- Browser test: the example drive shows VehicleSpeed peaking near 80 km/h, and clicking that point opens a VehicleStatus frame whose decoded speed matches.

---

## Phase 5: visual compare and exports

- For each changed message, draw the old and new bit grids side by side, coloring signals by name across both. Outline moved or resized signals, hatch removed ones, mark added ones, and list the breaking changes beside the grids they affect.
- "Compare with opened file" diffs the original against the edits from the Source tab in one click.
- Exports: lint as SARIF and JSON, diff as JSON, generated code as single files plus "Download all" through a small store-only zip writer (no library), and the decoded log as CSV.

### Acceptance

- Browser test: with the example revisions, Status.Counter visibly moves from bits 56-59 to 60-63, and the downloaded zip contains both C files.

---

## Phase 6: first-time visitors and polish

- A first-visit intro, dismissible and remembered: two sentences on what a DBC is (the car's computers share one wire, and the DBC file is the codebook for which bits mean what), then three actions in order: flip a bit, see a problem, see the generated C.
- Code view: line numbers and lightweight syntax highlighting for C and Python using a small tokenizer, no library.
- Bit grid keyboard control: arrow keys move between bits, Space flips the focused bit, Home and End jump within a byte.
- Problems: filter by severity and group by message.
- A responsive pass down to 375 px wide, WCAG AA contrast in both themes, and a keyboard-only walkthrough of every view.
- Report the WebAssembly module size in CI and keep it under 500 KB.
- README: a short animated capture of the demo, plus an Open Graph preview image for link sharing.

---

## Not now

- Generating Rust code (worth its own plan later)
- Extended multiplexing (`SG_MUL_VAL_`)
- Vector ASC or BLF logs, and live hardware through WebSerial or WebUSB
- Accounts, servers, or uploads of any kind

## WebAssembly exports added by this plan

| Export | Phase | Returns |
|---|---|---|
| `cf_load(src)` | 1 | Analysis JSON; keeps the database loaded |
| `cf_decode_loaded(frame_id, hex)` | 1 | Decoded frame |
| `cf_generate_loaded(lang, prefix, source_name)` | 1 | Generated files |
| `cf_signal_code_loaded(message, signal, prefix)` | 1 | C for one signal |
| `cf_encode_loaded(frame_id, base_hex, assignments)` | 3 | Hex, clamps, ignored values |
| `cf_decode_log_loaded(text)` | 4 | Columnar signal series |
