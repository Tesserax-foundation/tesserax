# Tessera

Tessera is a proof-of-work blockchain built with the Polkadot SDK (Substrate) as an independent
chain. Blocks are mined on CPUs with RandomTESS, Tessera's configuration of RandomX v2.

## Repository layout

| Path | Description |
|---|---|
| `node/` | The `tessera` node: networking, block import, built-in miner, RPC |
| `runtime/` | The on-chain logic, compiled to WebAssembly |
| `pallets/difficulty/` | Difficulty adjustment (LWMA over 120 blocks, 18 s target) |
| `consensus/pow/` | RandomTESS proof of work: seal verification, key schedule, miner, confirmation depth |
| `crypto/randomtess/` | RandomTESS hash function (RandomX v2 sources with the Tessera configuration) |

## Consensus

- Target block time: 18 seconds.
- A block is valid when `RandomTESS(key, pre_hash || nonce)`, read as a little-endian 256-bit
  integer, times the difficulty does not overflow 256 bits.
- The RandomTESS key changes every 2048 blocks and is the hash of a block buried 64 blocks deep.
- The difficulty is recomputed every block by the runtime from the last 120 solve times.
- The heaviest chain wins. Blocks 60 deep in the best chain are finalized: deeper
  reorganizations are rejected.

## Building

Requirements: Rust (installed through `rustup`, the toolchain is pinned by `rust-toolchain.toml`),
a C++ compiler, CMake, `protoc` and libclang.

On macOS, libclang ships with the Xcode command line tools but is not on the default library path:

```bash
export LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib
export DYLD_FALLBACK_LIBRARY_PATH=/Library/Developer/CommandLineTools/usr/lib
```

```bash
cargo build --release
```

## Running a development chain

```bash
./target/release/tessera --dev --mine --author 5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY
```

`--mine` starts the built-in CPU miner, `--author` is the account credited for mined blocks.
`--mining-threads N` sets the number of threads (all cores by default) and `--mining-light`
mines with 256 MiB of memory instead of 2 GiB, at a much lower hashrate.

## Tests

```bash
cargo test --workspace
```
