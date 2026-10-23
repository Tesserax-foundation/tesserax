#!/bin/sh
# Entry point of docker/Dockerfile.devnet: compiles the node from the sources of the image unless
# the volume already holds the binary of these sources, then runs it with the arguments given.
#
#   DATA  volume of the node (/data by default), where the binary is kept in bin/
#
# The node is compiled without its development runtime (SKIP_WASM_BUILD): a node started from a
# chain specification runs the runtime of the chain.

set -eu

DATA=${DATA:-/data}
binary="$DATA/bin/tesserax-$(cat /build/.source-id)"

if [ ! -x "$binary" ]; then
	echo "Compiling the node, which takes a while"
	cd /build
	SKIP_WASM_BUILD=1 CARGO_TARGET_DIR=/tmp/target cargo build --release --locked -p tesserax-node
	mkdir -p "$DATA/bin"
	rm -f "$DATA"/bin/tesserax-*
	cp /tmp/target/release/tesserax "$binary.partial"
	mv "$binary.partial" "$binary"
	rm -rf /tmp/target
	echo "Node compiled: $binary"
fi

exec "$binary" "$@"
