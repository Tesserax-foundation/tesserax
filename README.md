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
| `pool/` | Mining pool: stratum server for XMRig rigs, verified shares, PPLNS rewards and payouts |
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
| `pallets/swaps/` | Atomic swaps of TESS against ETH, USDC and USDT on EVM chains, see `docs/swaps.md` |
| `swaps/evm/` | Hash time-locked contract of the swaps on Ethereum, Base and Arbitrum |
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

The Rust, TypeScript and Python SDKs deploy contracts, call them by function name, read them
without a transaction and follow their events: see [docs/contracts.md](docs/contracts.md), from the
Solidity to the deployment.

## Governance

There is no sudo key. Referenda decide, a runtime upgrade included: holders of TESS vote with
conviction, and TESS still vesting do not vote, so that the allocations of the team and of the
foundation decide nothing while they vest. The technical committee, the three signatories of the
foundation, can only whitelist a fix for a shorter referendum, and its members never change. See
[docs/governance.md](docs/governance.md).

## Swaps

TESS trade against ETH, USDC and USDT on Ethereum, Base and Arbitrum without an exchange: makers
offer TESS on Tesserax, takers lock their payment on the EVM chain, and one secret releases both
sides, or both return to their owners. A taker needs no TESS to receive them. See
[docs/swaps.md](docs/swaps.md).

## Security

Vulnerabilities are rewarded in TESS, 25,000 TESS to start: report them privately with
**Security → Report a vulnerability** on this repository. See [SECURITY.md](SECURITY.md) and
[docs/bug-bounty.md](docs/bug-bounty.md).

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

`tesserax-pool` (`pool/`) runs a public mining pool on top of a node: verified shares, PPLNS
rewards, automatic payouts, a dashboard and an API. See [docs/pool.md](docs/pool.md).

The public testnet, the one the explorer and the web wallet follow, is described in
[docs/testnet.md](docs/testnet.md): `tesserax --chain testnet` joins it. Its public RPC, `wss://rpc.tesserax.app:21207`, is served by
`tesserax-rpc-gateway` (`rpc-gateway/`) with post-quantum TLS: see [docs/rpc.md](docs/rpc.md).

## Tests

```bash
cargo test --workspace
```

## Releases and CI

- `.github/workflows/ci.yml`, on every push to `main` and every pull request: builds the node,
  the wallet and the pool, runs every Rust test, checks that the benchmarks compile, then starts
  a development chain with the node just built and runs the TypeScript and Python SDKs and the
  explorer indexer against it. A push to `main` that passes is published as the Docker image
  `ghcr.io/<owner>/tesserax:main`.
- `.github/workflows/release.yml`, on a tag `v<version>` matching `node/Cargo.toml`: the
  runtime built reproducibly, the node, the wallet and the pool for Linux and macOS, a Docker
  image on the GitHub container registry, and a draft release with their checksums.

The runtime of a release is built by `scripts/build-runtime.sh`, in a Rust image pinned by its
digest with the toolchain of `rust-toolchain.toml`: anyone can rebuild it and check its
BLAKE2-256 hash, the one a runtime upgrade carries.

```bash
scripts/build-runtime.sh          # target/runtime-release/: the runtime and its hashes
docker build -t tesserax .        # the node, the wallet and the pool from the sources
```

## Weights

Every call of the runtime has a weight, the time it takes, from which its fee follows: the
fixed costs of a block and of a transaction (ML-DSA-87 signature check included), and a
benchmarked weight for each call of every pallet, those of Tesserax and those of the Polkadot
SDK in this runtime. `scripts/benchmark.sh` measures them all and writes them to the sources
(`pallets/*/src/weights.rs`, `runtime/src/weights/`):

```bash
cargo build --release -p tesserax-node --features runtime-benchmarks
scripts/benchmark.sh
```

Weights are times on the machine that measures them: run it on the reference hardware of the
network before a release, on an idle machine. `PALLETS="pallet_tasks"` measures one pallet,
`STEPS` and `REPEAT` set the precision.

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
