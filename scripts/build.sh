#!/usr/bin/env bash
# Build the wasm binary and generate JS bindings for both browser (web)
# and Node targets. Outputs land in pkg/ and pkg-node/.
set -euo pipefail

cargo build --target wasm32-unknown-unknown --release

WASM=target/wasm32-unknown-unknown/release/bagr_wasm.wasm

wasm-bindgen --target web    --out-dir pkg      --out-name bagr_wasm "$WASM"
wasm-bindgen --target nodejs --out-dir pkg-node --out-name bagr_wasm "$WASM"

echo
echo "Built:"
ls -la pkg/bagr_wasm_bg.wasm pkg-node/bagr_wasm_bg.wasm
