#!/bin/sh
# Assemble the browser demo in site/: every file in web/, the WebAssembly
# module, and the example files the page fetches. CI and scripts/dev-site.sh
# both run this script, so the site you test locally is the site CI deploys.
#
# Build the module first:
#   cargo build --release --lib --target wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")/.."

wasm=target/wasm32-unknown-unknown/release/canforge.wasm
if [ ! -f "$wasm" ]; then
  echo "assemble-site.sh: $wasm is missing." >&2
  echo "Build it with: cargo build --release --lib --target wasm32-unknown-unknown" >&2
  exit 1
fi

# Start from an empty folder so files deleted from web/ do not linger.
rm -rf site
mkdir -p site/examples
cp -R web/. site/
cp "$wasm" site/
cp examples/powertrain.dbc tests/fixtures/diff/v1.dbc tests/fixtures/diff/v2.dbc site/examples/

echo "Assembled site/:"
(cd site && find . -type f | sort | sed 's|^\./|  |')
