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
| `pallets/emission/` | Block rewards: 0.85 TESS per block to the miner, halving every 7 000 000 blocks |
| `pallets/shielded/` | Shielded pool: private transfers proven with zero-knowledge STARKs, see `docs/privacy.md` |
| `consensus/pow/` | RandomTESS proof of work: seal verification, key schedule, miner, confirmation depth |
| `primitives/crypto/` | ML-DSA-65 keys, signatures and accounts |
| `primitives/shielded/` | Notes, note tree, note encryption and the STARK circuit of the shielded pool |
| `vendor/` | Forks of Polkadot SDK crates for post-quantum networking and contracts, see `vendor/README.md` |
| `crypto/randomtess/` | RandomTESS hash function (RandomX v2 sources with the Tessera configuration) |

## Consensus

- Target block time: 18 seconds.
- A block is valid when `RandomTESS(key, pre_hash || nonce)`, read as a little-endian 256-bit
  integer, times the difficulty does not overflow 256 bits.
- The RandomTESS key changes every 2048 blocks and is the hash of a block buried 64 blocks deep.
- The difficulty is recomputed every block by the runtime from the last 120 solve times.
- The heaviest chain wins. Blocks 60 deep in the best chain are finalized: deeper
  reorganizations are rejected.

## Emission

The supply is 14 000 000 TESS (12 decimals):

| Share | Amount | Release |
|---|---|---|
| Mining, 85% | 11 900 000 TESS | Block rewards (`pallets/emission`) |
| Team, 10% | 1 400 000 TESS | Linear vesting over 48 months, no cliff |
| Foundation, 5% | 700 000 TESS | 140 000 at genesis, 560 000 linearly over 36 months |

The team and foundation allocations are part of the genesis of the public network. Development
chains endow test accounts instead.

- Each block mints 0.85 TESS to its miner, the account in the proof-of-work pre-runtime digest,
  which the seal covers. The genesis block has no reward.
- The reward halves every 7 000 000 blocks (about four years): 0.425 TESS from block 7 000 001,
  0.2125 TESS from block 14 000 001, and so on. The halvings mint at most 11 900 000 TESS; the
  runtime also caps the total minted at that amount.
- Nothing is burned. The miner of a block also receives the fees and tips of its transactions,
  the dust of accounts removed below the existential deposit (0.001 TESS) and the rounding of
  contract gas.
- Value in the shielded pool is not counted in the total issuance of the balances pallet: the
  total supply is the total issuance plus the pool balance (see `docs/privacy.md`).

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

## Smart contracts

Contracts are written in Solidity, compiled to PolkaVM (RISC-V) with
[`resolc`](https://github.com/paritytech/revive) and executed by `pallet-revive`. There is no EVM:

- only PolkaVM code can be uploaded, EVM bytecode is rejected;
- contracts are deployed and called with ML-DSA signed extrinsics
  (`revive.instantiate_with_code`, `revive.call`); Ethereum transactions are rejected;
- precompiles based on elliptic curves (`ecrecover`, `bn128`, `p256` verification, KZG point
  evaluation) and the sr25519/ECDSA helpers of the contract API are disabled;
- contracts can verify ML-DSA-65 signatures through the precompile at
  `0x00000000000000000000000000000000d5a00000`:

```solidity
interface IMlDsa65 {
    function verify(
        bytes calldata publicKey,
        bytes calldata context,
        bytes calldata message,
        bytes calldata signature
    ) external view returns (bool);
}
```

  The FIPS 204 context `"tessera"` is reserved for transactions and always fails.

Every ML-DSA account has a contract address (`msg.sender`), the last 20 bytes of the Keccak-256
hash of its account id. The mapping is registered automatically when the account is created.

```bash
npx @parity/resolc --bin --abi -o out MyContract.sol
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
