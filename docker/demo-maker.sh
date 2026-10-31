#!/bin/sh
# Start command of the demo maker of the testnet (docs/testnet.md), on docker/Dockerfile.from-source:
# a maker selling TESS for testnet4 BTC, so that the market of the wallet always has an offer, and,
# when given bids to keep, buying TESS with the testnet4 BTC of its key, so that sellers always find
# a bid.
#
# Its account, the password of its keystore and its Bitcoin key are created on its first start and
# kept in the volume; they never leave the service. Send it TESS from another account: it offers
# them again as they sell.
#
#   NODE_URL     node it reads and submits to (ws://node.internal:9944, say)
#   KEEP_OFFER   offers kept open, chain:asset:price:amount:min (see `swap make --help`), several
#                separated by spaces; bitcoin:btc:0.00001:1000:10 by default
#   KEEP_BID     bids kept open, chain:asset:price:amount:min, several separated by spaces; none
#                by default. The sales are paid from the Bitcoin key: fund its address first.
#   DATA         volume (/data by default)

set -eu

DATA=${DATA:-/data}
KEEP_OFFER=${KEEP_OFFER:-bitcoin:btc:0.00001:1000:10}
KEEP_BID=${KEEP_BID:-}
export PACKAGE=tesserax-wallet BINARY=tesserax-wallet DATA
export TESSERAX_URL="$NODE_URL" TESSERAX_WALLET_DIR="$DATA/wallet"

if [ ! -s "$DATA/maker.password" ]; then
	(umask 077 && head -c 32 /dev/urandom | base64 > "$DATA/maker.password")
fi
TESSERAX_PASSWORD=$(cat "$DATA/maker.password")
export TESSERAX_PASSWORD

# Compiles the wallet on the first start of these sources.
/build/docker/from-source.sh --version
if ! /build/docker/from-source.sh account show maker >/dev/null 2>&1; then
	# The phrase is not printed: the keystore in the volume is the account.
	/build/docker/from-source.sh account new maker >/dev/null 2>&1
fi
echo "Maker account: $(/build/docker/from-source.sh account show maker)"

set --
for offer in $KEEP_OFFER; do
	set -- "$@" --keep-offer "$offer"
done
for bid in $KEEP_BID; do
	set -- "$@" --keep-bid "$bid"
done
exec /build/docker/from-source.sh swap make maker --interval 30 --btc-key-file "$DATA/btc.wif" --create-btc-key "$@"
