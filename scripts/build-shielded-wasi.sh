#!/usr/bin/env bash
# Builds the shielded wallet for WASI hosts (shielded-wasi) into the Python SDK, which runs it
# with wasmtime:
#
#   scripts/build-shielded-wasi.sh [output file]
#
# Requires the wasm32-wasip1 target (rust-toolchain.toml installs it).

set -euo pipefail
cd "$(dirname "$0")/.."

OUT=${1:-sdk-python/src/tesserax/shielded.wasm}

# Paths of this machine stay out of the module (panic locations): sources, registry, toolchain.
export RUSTFLAGS="--remap-path-prefix=$PWD=/tesserax --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup"
cargo build --release --locked --target wasm32-wasip1 -p tesserax-shielded-wasi
cp target/wasm32-wasip1/release/tesserax_shielded_wasi.wasm "$OUT"
ls -l "$OUT"
