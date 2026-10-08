#!/bin/sh
# Build the WebAssembly module, assemble site/ exactly as CI does, and serve
# it at http://127.0.0.1:8000/. Set PORT to use another port. Run it again
# after changing web/ or the Rust code; site/ is a copy, not a link.
set -eu
cd "$(dirname "$0")/.."

cargo build --release --lib --target wasm32-unknown-unknown
scripts/assemble-site.sh

port="${PORT:-8000}"
echo "Serving site/ at http://127.0.0.1:$port/ (press Ctrl-C to stop)"
exec python3 -m http.server --bind 127.0.0.1 --directory site "$port"
