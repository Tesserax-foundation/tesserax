#!/usr/bin/env bash
# Builds the shielded wallet for JavaScript (shielded-wasm) into a directory, by default the one
# the web wallet vendors:
#
#   scripts/build-shielded-wasm.sh [output directory]
#   scripts/build-shielded-wasm.sh sdk-ts/wasm      # the one of the TypeScript SDK
#
# Requires wasm-pack and the wasm32-unknown-unknown target.

set -euo pipefail
cd "$(dirname "$0")/.."

OUT=${1:-../tesserax-wallet/vendor/tesserax-shielded}
OUT=$(mkdir -p "$OUT" && cd "$OUT" && pwd)

# Paths of this machine stay out of the module (panic locations): sources, registry, toolchain.
# RUSTFLAGS replaces the flags of shielded-wasm/.cargo/config.toml, repeated here.
export RUSTFLAGS="--cfg getrandom_backend=\"wasm_js\" --remap-path-prefix=$PWD=/tesserax --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup"
(cd shielded-wasm && wasm-pack build --target web --release --no-pack --out-dir "$OUT")
rm -f "$OUT/.gitignore"
ls -l "$OUT"
