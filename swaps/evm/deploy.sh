#!/usr/bin/env bash
# Deploys TesseraxSwap with the deterministic CREATE2 deployer, which exists on Ethereum, Base,
# Arbitrum and their testnets: the contract has the same address on every chain.
#
#   RPC_URL=https://mainnet.base.org ./deploy.sh --account deployer
#
# The arguments are those of `cast send` to sign: --account, --private-key, --ledger... The
# deployment is idempotent: it does nothing where the contract already is.

set -euo pipefail
cd "$(dirname "$0")"

DEPLOYER=0x4e59b44847b379578588920ca78fbf26c0b4956c
SALT=$(cast keccak "tesserax-swap-v1")
INIT=$(forge inspect TesseraxSwap bytecode)
ADDRESS=$(cast create2 --deployer "$DEPLOYER" --salt "$SALT" --init-code-hash "$(cast keccak "$INIT")")

if [ "$(cast code "$ADDRESS" --rpc-url "$RPC_URL")" != "0x" ]; then
	echo "TesseraxSwap is already at $ADDRESS"
	exit 0
fi
cast send "$DEPLOYER" "${SALT}${INIT#0x}" --rpc-url "$RPC_URL" "$@"
[ "$(cast code "$ADDRESS" --rpc-url "$RPC_URL")" != "0x" ] || { echo "not deployed" >&2; exit 1; }
echo "TesseraxSwap deployed at $ADDRESS"
