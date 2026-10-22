#!/usr/bin/env bash
# Measures the weights of the runtime and writes them to the sources: the weights of the pallets
# of Tesserax (pallets/*/src/weights.rs), those of the Polkadot SDK pallets in this runtime
# (runtime/src/weights/), and the fixed costs of a block and of an ML-DSA-87 signed transaction.
#
# Weights are times on the machine that runs this script: run it on the reference hardware of
# the network before a release, with the node built with the benchmarks:
#
#   cargo build --release -p tesserax-node --features runtime-benchmarks
#   scripts/benchmark.sh
#
# STEPS and REPEAT set the precision (50 and 20 by default); PALLETS restricts the run, such as
# PALLETS="pallet_tasks pallet_channels".

set -euo pipefail
cd "$(dirname "$0")/.."

NODE=${NODE:-target/release/tesserax}
RUNTIME=${RUNTIME:-target/release/wbuild/tesserax-runtime/tesserax_runtime.compact.compressed.wasm}
STEPS=${STEPS:-50}
REPEAT=${REPEAT:-20}

TESSERAX_PALLETS="pallet_agents pallet_agent_registry pallet_channels pallet_difficulty pallet_emission pallet_shielded pallet_tasks"
SDK_PALLETS="frame_system frame_system_extensions pallet_balances pallet_timestamp pallet_sudo pallet_multisig pallet_vesting pallet_transaction_payment"
# A run restricted to some pallets leaves the base weights as they are.
OVERHEAD=${PALLETS:+no}
PALLETS=${PALLETS:-"$TESSERAX_PALLETS $SDK_PALLETS"}

for file in "$NODE" "$RUNTIME"; do
	if [ ! -f "$file" ]; then
		echo "$file is missing: cargo build --release -p tesserax-node --features runtime-benchmarks" >&2
		exit 1
	fi
done

bench() {
	local pallet=$1 template=$2 output=$3
	echo "Benchmarking $pallet"
	"$NODE" benchmark pallet \
		--runtime "$RUNTIME" \
		--genesis-builder-preset development \
		--pallet "$pallet" --extrinsic '*' \
		--steps "$STEPS" --repeat "$REPEAT" \
		--wasm-execution compiled \
		--heap-pages 4096 \
		--template "$template" \
		--output "$output"
}

for pallet in $PALLETS; do
	case " $TESSERAX_PALLETS " in
		*" $pallet "*)
			directory=pallets/$(echo "${pallet#pallet_}" | tr _ -)
			bench "$pallet" scripts/benchmarks/pallet.hbs "$directory/src/weights.rs"
			;;
		*)
			bench "$pallet" scripts/benchmarks/runtime.hbs "runtime/src/weights/$pallet.rs"
			;;
	esac
done

if [ -z "$OVERHEAD" ]; then
	echo "Benchmarking the base weights of a block and of a transaction"
	"$NODE" benchmark overhead --dev --weight-path runtime/src/weights
fi

# The machine is described by its processor only, which the CLI does not always find.
cpu=$( (sysctl -n machdep.cpu.brand_string 2>/dev/null || grep -m1 "model name" /proc/cpuinfo | cut -d: -f2) | sed 's/^ *//')
sed -i.bak -E -e '/HOSTNAME/d' -e "s|<UNKNOWN>|${cpu:-unknown processor}|" runtime/src/weights/*.rs pallets/*/src/weights.rs
rm -f runtime/src/weights/*.rs.bak pallets/*/src/weights.rs.bak
echo "Weights written. Review the changes with git diff before committing them."
