#!/usr/bin/env bash
# Builds the shielded wallet for JavaScript (shielded-wasm) into a directory, by default the one
# the web wallet vendors:
#
#   scripts/build-shielded-wasm.sh [output directory]
#
# Requires wasm-pack and the wasm32-unknown-unknown target.

set -euo pipefail
cd "$(dirname "$0")/.."

OUT=${1:-../tesserax-wallet/vendor/tesserax-shielded}
OUT=$(mkdir -p "$OUT" && cd "$OUT" && pwd)

(cd shielded-wasm && wasm-pack build --target web --release --no-pack --out-dir "$OUT")
rm -f "$OUT/.gitignore"
ls -l "$OUT"
