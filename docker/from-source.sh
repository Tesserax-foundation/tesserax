#!/bin/sh
# Entry point of docker/Dockerfile.from-source: compiles PACKAGE from the sources of the image
# unless the volume already holds the binary of these sources, then runs it with the arguments
# given.
#
#   PACKAGE  package to compile (tesserax-node by default)
#   BINARY   binary of the package (tesserax by default)
#   DATA     volume, where the binary is kept in bin/ (/data by default)
#
# The node is compiled without its development runtime (SKIP_WASM_BUILD): a node started from a
# chain specification runs the runtime of the chain.

set -eu

PACKAGE=${PACKAGE:-tesserax-node}
BINARY=${BINARY:-tesserax}
DATA=${DATA:-/data}
binary="$DATA/bin/$BINARY-$(cat /build/.source-id)"

if [ ! -x "$binary" ]; then
	echo "Compiling $PACKAGE, which takes a while"
	cd /build
	SKIP_WASM_BUILD=1 CARGO_TARGET_DIR=/tmp/target cargo build --release --locked -p "$PACKAGE"
	mkdir -p "$DATA/bin"
	rm -f "$DATA/bin/$BINARY"-*
	cp "/tmp/target/release/$BINARY" "$binary.partial"
	mv "$binary.partial" "$binary"
	rm -rf /tmp/target
	echo "Compiled: $binary"
fi

exec "$binary" "$@"
