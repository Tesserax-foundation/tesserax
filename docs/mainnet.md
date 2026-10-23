# Mainnet

The genesis of the mainnet is the `mainnet` preset of the runtime
(`runtime/src/genesis_config_presets.rs`). Its specification does not exist yet: it is generated
once, at the launch, from the runtime of the release, then frozen in `chain-specs/mainnet.json`.

## Genesis

| | |
|---|---|
| Supply | 14,000,000 TESS: 85% mined, 10% team, 5% foundation |
| Mining | 0.85 TESS a block of 18 seconds, halving every 7,000,000 blocks |
| Team | 1,400,000 TESS, half to each member, released from a six-month cliff to 48 months |
| Foundation | 700,000 TESS to a 2-of-3 multisig, 140,000 liquid and the rest released over 36 months |
| Sudo | the multisig of the foundation |
| Difficulty | 1,000,000 at genesis, about 55 kH/s at 18 seconds a block |
| Other balances | none: every other TESS is mined |

Accounts:

| Account | Address |
|---|---|
| Team member 1, foundation signatory | `tetTtAxXxBzRTTsc5otocLgRrGZVCcxzBeK4NSGwdqAsee7XM` |
| Team member 2, foundation signatory | `tetBiN5GwskQqq8ykcMUQbqCPdWPRaE5PoYwfazsJ2jHGtTZw` |
| Backup key, foundation signatory | `terN4gSZpiFpeERvb1PoaJiiBRqSEmXw56xF5MqqjLbp7eH3x` |
| Foundation, sudo | the 2-of-3 multisig of the three signatories |

Any two of the three keys approve a spending of the foundation or a sudo call; losing one key
blocks nothing, and one stolen key is not enough. Keep the backup key offline.

The test `mainnet_genesis_creates_the_allocations_only` (`runtime/src/tests/allocations.rs`)
checks these balances, the cliff of the team, the liquid part of the foundation and the sudo key.

## Initial difficulty

The difficulty follows the miners from the first blocks (LWMA over 120 blocks), but its value at
genesis decides the first hour. Too low, the first miners find blocks in seconds and take a large
share of the early emission until the difficulty catches up; too high, the first blocks are slow
until it comes down. Set `MAINNET_INITIAL_DIFFICULTY` just before the launch to the hashrate
expected at the start, in hashes per second, times 18: the hashrate of the testnet is a good
measure.

## Launch

1. Freeze the runtime of the release and build it reproducibly: `scripts/build-runtime.sh` writes
   `target/runtime-release/tesserax_runtime.compact.compressed.wasm` and its BLAKE2-256 hash.
   Publish the hash with the release, so that anyone can rebuild the runtime and compare.
2. Generate the specification with that runtime:

   ```bash
   tesserax export-chain-spec --chain mainnet-genesis > mainnet-plain.json
   # Replace genesis.runtimeGenesis.code by 0x and the hexadecimal of the reproducible runtime.
   tesserax export-chain-spec --chain mainnet-plain.json --raw > chain-specs/mainnet.json
   ```

3. Add the bootnodes to `bootNodes`, make the node load `chain-specs/mainnet.json` for
   `--chain mainnet`, as it does for the testnet, and release the node.
4. Announce the genesis hash and the launch time; the first block is mined by whoever starts
   first.

## Sudo through the multisig

A sudo call, such as a runtime upgrade (`system.set_code`), is wrapped twice: `sudo.sudo(call)`,
then `multisig.as_multi` by a first signatory and `multisig.approve_as_multi` or `as_multi` by a
second one. The foundation can later remove sudo (`sudo.remove_key`) for an on-chain governance.
