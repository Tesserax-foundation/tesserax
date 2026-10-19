# Tessera

Tessera is a proof-of-work blockchain built with the Polkadot SDK (Substrate) as an independent
chain. Blocks are mined on CPUs with RandomTESS, Tessera's configuration of RandomX v2, and every
transaction is signed with ML-DSA (FIPS 204), a post-quantum signature scheme.

## Repository layout

| Path | Description |
|---|---|
| `node/` | The `tessera` node: networking, block import, built-in miner, RPC |
| `runtime/` | The on-chain logic, compiled to WebAssembly |
| `pallets/difficulty/` | Difficulty adjustment (LWMA over 120 blocks, 18 s target) |
| `consensus/pow/` | RandomTESS proof of work: seal verification, key schedule, miner, confirmation depth |
| `primitives/crypto/` | ML-DSA-65 keys, signatures and accounts |
| `crypto/randomtess/` | RandomTESS hash function (RandomX v2 sources with the Tessera configuration) |

## Consensus

- Target block time: 18 seconds.
- A block is valid when `RandomTESS(key, pre_hash || nonce)`, read as a little-endian 256-bit
  integer, times the difficulty does not overflow 256 bits.
- The RandomTESS key changes every 2048 blocks and is the hash of a block buried 64 blocks deep.
- The difficulty is recomputed every block by the runtime from the last 120 solve times.
- The heaviest chain wins. Blocks 60 deep in the best chain are finalized: deeper
  reorganizations are rejected.

## Accounts and signatures

ML-DSA-65 is the only signature scheme of the chain: there are no sr25519, ed25519 or ECDSA
accounts.

- An account id is the BLAKE2-256 hash of an ML-DSA-65 public key.
- A transaction carries the signer's public key (1952 bytes) and an ML-DSA signature (3309 bytes)
  made with the FIPS 204 context string `tessera`. It is valid when the key hashes to the sender
  and the signature verifies.
- Addresses use the SS58 format 2261 and start with `te`.
- Keys are derived from 24 word BIP39 phrases (256 bits of entropy). Only hard derivation
  (`//name`) is supported.

```bash
./target/release/tessera key generate
./target/release/tessera key inspect "//Alice"
```

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
./target/release/tessera --dev --mine --author tesCrqHdUnYBar3RrUCeRt865AjG3UXifLKYvGKvR7asQJpVX
```

`--mine` starts the built-in CPU miner, `--author` is the account credited for mined blocks (here
the development account `//Alice`).
`--mining-threads N` sets the number of threads (all cores by default) and `--mining-light`
mines with 256 MiB of memory instead of 2 GiB, at a much lower hashrate.

## Tests

```bash
cargo test --workspace
```
