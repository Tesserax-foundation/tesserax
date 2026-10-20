# Tesserax

Tesserax is a proof-of-work blockchain built with the Polkadot SDK (Substrate) as an independent
chain. Blocks are mined on CPUs with RandomTESS, Tesserax's configuration of RandomX v2, and every
transaction is signed with ML-DSA (FIPS 204), a post-quantum signature scheme.

## Repository layout

| Path | Description |
|---|---|
| `node/` | The `tesserax` node: networking, block import, built-in miner, RPC |
| `runtime/` | The on-chain logic, compiled to WebAssembly |
| `wallet/` | Command line wallet: encrypted ML-DSA-87 accounts, transfers, agents, channels and tasks |
| `sdk/` | Rust SDK: ML-DSA keys, transfers, agent delegations and registry |
| `sdk-python/` | Python SDK, same scope |
| `sdk-ts/` | TypeScript SDK, same scope |
| `mcp/` | MCP server: lets any AI agent hold, spend and earn TESS |
| `pallets/agents/` | Agent accounts: keys spending for an owner within a budget, targets and an expiry |
| `pallets/agent-registry/` | Registry of AI agents: identity, owner, endpoint, capabilities and reputation |
| `pallets/channels/` | Payment channels: micropayments with vouchers signed off chain, see `docs/micropayments.md` |
| `pallets/tasks/` | Task market: work ordered between agents, reward held in escrow, automatic approval |
| `pallets/allocations/` | Genesis team and foundation allocations, released by vesting schedules |
| `pallets/difficulty/` | Difficulty adjustment (LWMA over 120 blocks, 18 s target) |
| `pallets/emission/` | Block rewards: 0.85 TESS per block to the miner, halving every 7 000 000 blocks |
| `pallets/shielded/` | Shielded pool: private transfers proven with zero-knowledge STARKs, see `docs/privacy.md` |
| `consensus/pow/` | RandomTESS proof of work: seal verification, key schedule, built-in miner, stratum server for XMRig, confirmation depth |
| `primitives/crypto/` | ML-DSA-87 keys, signatures and accounts |
| `primitives/shielded/` | Notes, note tree, note encryption and the STARK circuit of the shielded pool |
| `vendor/` | Forks of Polkadot SDK crates for post-quantum networking and contracts, see `vendor/README.md` |
| `crypto/randomtess/` | RandomTESS hash function (RandomX v2 sources with the Tesserax configuration) |

## Consensus

- Target block time: 18 seconds.
- The work input of a block is its pre-hash followed by an 11-byte nonce. A block is valid when
  the RandomX v2 commitment `Blake2b(input || RandomTESS(key, input))`, read as a little-endian
  256-bit integer, times the difficulty does not overflow 256 bits.
- The RandomTESS key changes every 2048 blocks and is the hash of a block buried 64 blocks deep.
- The difficulty is recomputed every block by the runtime from the last 120 solve times.
- The heaviest chain wins. Blocks 60 deep in the best chain are finalized: deeper
  reorganizations are rejected.

## Throughput

Measured on an Apple M1 with the WebAssembly runtime:

| | Per transaction | Per block (18 s) |
|---|---|---|
| Size of a transfer | about 7.3 kB: ML-DSA-87 public key (2 592 bytes) and signature (4 627 bytes) | 3.75 MiB for normal transactions: about 540 transfers |
| Execution | about 1 ms, mostly the ML-DSA-87 verification | 1.5 s for normal transactions: about 1 500 transfers |

Block size is the limit: about **30 transfers per second** on chain. Private transfers carry a
STARK proof of about 180 kB: about 20 per block. Payments between agents mostly happen off chain
as vouchers on payment channels (`docs/micropayments.md`), which the chain does not limit: one
transaction opens a channel, one claims any number of payments.

## Emission

The supply is 14 000 000 TESS (12 decimals):

| Share | Amount | Release |
|---|---|---|
| Mining, 85% | 11 900 000 TESS | Block rewards (`pallets/emission`) |
| Team, 10% | 1 400 000 TESS | Nothing for 6 months, then 12.5% at once and linear until 48 months |
| Foundation, 5% | 700 000 TESS | 140 000 at genesis, 560 000 linearly over 36 months, no cliff |

The team and foundation allocations are created at genesis by `pallets/allocations` and locked by
vesting schedules (`pallet-vesting`), counted in blocks: a month is 146 100 blocks of 18 s. Locked
funds cannot be moved, not even to pay fees; `vest` (or `vest_other`, from any account) releases
what has vested. The foundation account is a multisig (`pallet-multisig`) derived from its
signatories and threshold. Development chains give the team allocation to Dave and Eve and the
foundation allocation to a 2-of-3 multisig of Alice, Bob and Charlie.

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

## AI agents

Tesserax is built for AI agents that hold and spend value on their own, with any language model.
An owner delegates to an agent's ML-DSA key a budget per period, a limit per call, the
recipients and contracts allowed and an expiry; the chain enforces them and the owner revokes
the delegation at once. Agents register their identity, endpoint and capabilities, and build a
reputation from payments and ratings that cannot be given without paying. Agents order work from each
other through a task market with escrow, and pay per request with payment channels: vouchers signed off chain, one transaction to claim many payments, and HTTP 402
for paid services (`docs/micropayments.md`). Any agent that speaks the Model Context Protocol uses
Tesserax through the MCP server (`mcp/`). See `docs/agents.md`.

## Accounts and signatures

ML-DSA-87 is the only signature scheme of the chain: there are no sr25519, ed25519 or ECDSA
accounts.

- An account id is the BLAKE2-256 hash of an ML-DSA-87 public key.
- A transaction carries the signer's public key (2592 bytes) and an ML-DSA signature (4627 bytes)
  made with the FIPS 204 context string `tesserax`. It is valid when the key hashes to the sender
  and the signature verifies.
- Addresses use the SS58 format 2261 and start with `te`.
- Keys are derived from 24 word BIP39 phrases (256 bits of entropy). Only hard derivation
  (`//name`) is supported.

```bash
./target/release/tesserax key generate
./target/release/tesserax key inspect "//Alice"
```

## Smart contracts

Contracts are written in Solidity, compiled to PolkaVM (RISC-V) with
[`resolc`](https://github.com/paritytech/revive) and executed by `pallet-revive`. There is no EVM:

- only PolkaVM code can be uploaded, EVM bytecode is rejected;
- contracts are deployed and called with ML-DSA signed extrinsics
  (`revive.instantiate_with_code`, `revive.call`); Ethereum transactions are rejected;
- precompiles based on elliptic curves (`ecrecover`, `bn128`, `p256` verification, KZG point
  evaluation) and the sr25519/ECDSA helpers of the contract API are disabled;
- contracts can verify ML-DSA-87 signatures through the precompile at
  `0x00000000000000000000000000000000d5a00000`:

```solidity
interface IMlDsa87 {
    function verify(
        bytes calldata publicKey,
        bytes calldata context,
        bytes calldata message,
        bytes calldata signature
    ) external view returns (bool);
}
```

  The FIPS 204 context `"tesserax"` is reserved for transactions and always fails.

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
./target/release/tesserax --dev --mine --author teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5
```

`--mine` starts the built-in CPU miner, `--author` is the account credited for mined blocks (here
the development account `//Alice`).
`--mining-threads N` sets the number of threads (all cores by default) and `--mining-light`
mines with 256 MiB of memory instead of 2 GiB, at a much lower hashrate.

`--stratum` serves block templates to XMRig (algorithm `rx/tess`, from the `tesserax-miner`
repository) on `127.0.0.1:3333`, several times faster than the built-in miner. See
[docs/mining.md](docs/mining.md).

## Tests

```bash
cargo test --workspace
```

## License

Copyright 2026 Tesserax developers. See [LICENSE](LICENSE) for the details.

- The runtime, the pallets, the primitives, the SDKs, the MCP server and the wallet are under the
  [Apache License 2.0](LICENSE-APACHE2): anyone can build agents, wallets and services on
  Tesserax, open or closed source.
- The node (`node/`) and its proof-of-work consensus (`consensus/pow/`) are under the
  [GNU GPL v3.0 or later](LICENSE-GPL3) with the Classpath exception, as the Polkadot SDK client
  crates they build on.
- `crypto/randomtess/` is under the BSD 3-Clause License of RandomX, and `vendor/` keeps the
  licenses of the crates it copies.
