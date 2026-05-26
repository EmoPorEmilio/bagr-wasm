#!/usr/bin/env bash
# Run every test suite: native Rust, the wasm bridge smoke test, the LoC
# conformance suite, and the cross-validation against bagit-python.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== Native cargo tests ==="
cargo test

echo
echo "=== Building wasm + JS bindings ==="
bash scripts/build.sh > /dev/null

echo
echo "=== Wasm bridge smoke test ==="
node tests/node-smoke/run.mjs > /tmp/smoke.log 2>&1 \
  && (grep -E '^\s*ok:' /tmp/smoke.log | wc -l | xargs -I{} echo "{} checks passed") \
  || (cat /tmp/smoke.log; exit 1)

echo
echo "=== LoC v0.97 conformance suite (vendored) ==="
node tests/conformance/run.mjs | tail -1

if command -v bagit >/dev/null 2>&1; then
  echo
  echo "=== Cross-validation vs bagit-python ==="
  node tests/cross-validate/run.mjs | tail -1
else
  echo
  echo "(skipping cross-validation — install bagit-python: pip install bagit)"
fi
