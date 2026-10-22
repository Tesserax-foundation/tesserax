#!/usr/bin/env bash
# Builds the runtime for an on-chain release, reproducibly: in a Rust image pinned by its digest,
# with the toolchain of rust-toolchain.toml, the locked dependencies and fixed paths, so that
# anyone can rebuild the WebAssembly runtime of a release and find the same hash.
#
#   scripts/build-runtime.sh            # writes target/runtime-release/
#
# Releases are built on x86-64 Linux (GitHub Actions). On another machine, Docker runs the
# same x86-64 image under emulation (PLATFORM=linux/amd64), which is slow but gives the same
# runtime. JOBS limits the parallel jobs when Docker has little memory.

set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE=docker.io/library/rust:1.99.1-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8
PLATFORM=${PLATFORM:-linux/amd64}
OUT=target/runtime-release

docker run --rm --platform "$PLATFORM" \
	--volume "$PWD":/build --workdir /build \
	--env CARGO_TARGET_DIR=/build/target/runtime-build \
	--env CARGO_TERM_COLOR=never ${JOBS:+--env CARGO_BUILD_JOBS=$JOBS} \
	--env IMAGE="$IMAGE" --env PLATFORM="$PLATFORM" --env OUT="$OUT" \
	"$IMAGE" \
	bash -euc '
		cargo build --release --locked -p tesserax-runtime --features on-chain-release-build
		mkdir -p "$OUT"
		wasm="$OUT/tesserax_runtime.compact.compressed.wasm"
		cp target/runtime-build/release/wbuild/tesserax-runtime/tesserax_runtime.compact.compressed.wasm "$wasm"
		spec=$(sed -n "s/^\t*spec_version: *\([0-9]*\),/\1/p" runtime/src/lib.rs | head -1)
		printf "{\n  \"spec_version\": %s,\n  \"image\": \"%s\",\n  \"platform\": \"%s\",\n  \"size\": %s,\n  \"blake2_256\": \"0x%s\",\n  \"sha256\": \"%s\"\n}\n" \
			"$spec" "$IMAGE" "$PLATFORM" "$(wc -c < "$wasm")" \
			"$(b2sum -l 256 "$wasm" | cut -d" " -f1)" "$(sha256sum "$wasm" | cut -d" " -f1)" \
			> "$OUT/tesserax_runtime.json"
	'
cat "$OUT/tesserax_runtime.json"
