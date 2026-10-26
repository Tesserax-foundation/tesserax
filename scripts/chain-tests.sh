#!/usr/bin/env bash
# Runs the TypeScript and Python SDKs and the explorer against a development chain mined by the
# node just built. These tests mine, so they run on our own machines, never on the hosted runners
# of a CI service, whose terms forbid mining.
#
#   cargo build --release -p tesserax-node && scripts/chain-tests.sh
#
# EVM_URL, with anvil and the swap contract deployed (swaps/evm/deploy.sh), adds the swaps.

set -euo pipefail
cd "$(dirname "$0")/.."

NODE=${NODE:-target/release/tesserax}
PORT=${PORT:-9944}
LOG=$(mktemp)
"$NODE" --dev --tmp --state-pruning archive --mine --mining-threads 2 --rpc-port "$PORT" \
	--author teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5 >"$LOG" 2>&1 &
NODE_PID=$!
trap 'kill $NODE_PID 2>/dev/null; rm -f "$LOG"' EXIT
for _ in $(seq 120); do
	curl -sf -H 'Content-Type: application/json' -d '{"id":1,"jsonrpc":"2.0","method":"chain_getHeader"}' \
		"http://127.0.0.1:$PORT" >/dev/null && break
	sleep 1
done
export TESSERAX_URL="ws://127.0.0.1:$PORT"

(cd sdk-ts && npm ci && npm run build && npm test) || { tail -100 "$LOG"; exit 1; }
(cd sdk-python && .venv/bin/python -m pytest) || { tail -100 "$LOG"; exit 1; }
(cd explorer && npm ci && npm run build && npm test) || { tail -100 "$LOG"; exit 1; }
echo "The SDKs and the explorer pass on a development chain."
