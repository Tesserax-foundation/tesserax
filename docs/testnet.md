# Testnet

The testnet is the public network of Tesserax ahead of the mainnet: the chain the explorer, the
public RPC and the web wallet follow. It has the tokenomics of the mainnet: the same emission, the
same allocations to the team and the foundation (test accounts) and the same fees. Its TESS are
test TESS: they have no value, and the testnet may be reset.

| | |
|---|---|
| Chain | `tesserax --chain testnet`, [`chain-specs/testnet.json`](../chain-specs/testnet.json) |
| Genesis hash | `0xc2f9cb03897c5ad30fff2f93debaa8b4a6e08f37a12270e107c71eb1ad39e5d1` |
| Bootnode | `/dns/boot.tesserax.app/tcp/28385/p2p/QmUAEp2bK2V3ADJkZym99dAEZZBN6WccwLecjfAFzpDWGo` |
| Public RPC | `wss://rpc.tesserax.app:21207`, post-quantum TLS ([rpc.md](rpc.md)) |
| Explorer | https://explorer.tesserax.app |
| Wallet | https://wallet.tesserax.app |
| Faucet | https://faucet.tesserax.app |
| Mining pool | https://pool.tesserax.app, stratum `stratum.tesserax.app:25202` |

The specification holds the genesis state itself, runtime included, and the bootnodes: the node
embeds it, and a node started with `--chain testnet` joins the testnet whatever the runtime it is
built with.

## Get test TESS

- **Faucet**: https://faucet.tesserax.app gives 10 test TESS per address every 24 hours.
- **Mining**: every block pays its miner 0.85 test TESS, as on the mainnet.

## Mine on the pool

The quickest way to mine: no node to run, only the miner.

1. Create an account in the [wallet](https://wallet.tesserax.app) and copy its address.
2. Download the Tesserax build of XMRig for your system from the
   [releases of `tesserax-miner`](https://github.com/Tesserax-foundation/tesserax-miner/releases) and unpack
   it. On macOS, allow it once: `xattr -d com.apple.quarantine xmrig`.
3. Start it with your address and a name for the machine:

   ```bash
   ./xmrig -a rx/tess -o stratum.tesserax.app:25202 -u <your address>.rig1 -p x
   ```

   On Windows: `xmrig.exe` instead of `./xmrig`. Mining at full speed takes about 2.5 GB of
   memory; on a machine with less, add `--randomx-mode=light`, several times slower.
4. Follow your hashrate and balance on https://pool.tesserax.app with your address. A block is
   credited once it is 60 blocks deep; balances of 1 test TESS or more are paid every 30 minutes.

The pool keeps 1% of each block for the fees of its payouts. See [pool.md](pool.md).

## Run a node

Build the node ([README](../README.md#building)), then:

```bash
tesserax --chain testnet                                   # follow the chain
tesserax --chain testnet --mine --author <your address>    # and mine with the CPU
```

`--mining-threads N` sets the number of threads. `--mining-light` mines with 256 MiB of memory
instead of 2 GiB, at a lower hashrate: on a machine with 8 GiB of memory or less, use it.

To mine with XMRig, several times faster than the built-in miner, run the node with `--stratum` and
point the Tesserax build of XMRig at it ([mining.md](mining.md)):

```bash
tesserax --chain testnet --stratum --author <your address>
./xmrig -o 127.0.0.1:3333 -u rig1 -a rx/tess
```

## The genesis

- Mining: 0.85 TESS a block, halving every 7,000,000 blocks.
- Team: 1,400,000 TESS, with a six-month cliff then linear vesting to 48 months.
- Foundation: 700,000 TESS to a 2-of-3 multisig, 140,000 liquid and the rest vested over 36
  months.
- Faucet: 100,000 test TESS to the operator, which funds the faucet. This one exists only on the
  testnet.
- Difficulty: 1,000 at genesis, the minimum; it follows the miners who join.

The operator account, the team account and the three foundation signatories are derived from the
secret phrase of the operator of the testnet: `//sudo` (the sudo key until runtime 103), `//team`
and `//foundation//1` to `//foundation//3`. Their addresses are in `runtime/src/genesis_config_presets.rs`.

The specification was generated once from the `testnet` preset of the runtime, then frozen:

```bash
tesserax export-chain-spec --chain testnet-genesis --raw > chain-specs/testnet.json
```

Generating it again with another runtime gives another genesis, so another chain.

## Runtime upgrades

The testnet started with a sudo key; since runtime 103 it has none, and its runtime changes by
referendum like the mainnet's ([governance.md](governance.md)): `tesserax-wallet governance
propose-upgrade` authorizes the WebAssembly runtime, whose `spec_version` is higher than the one on
chain, and `apply-upgrade` submits it once the referendum is enacted. Its technical committee is the three signatories of its foundation. Nodes run the runtime
of the chain whatever their version, and a node syncing from the genesis applies the upgrades in
their blocks.

| `spec_version` | Date | Change |
|---|---|---|
| 100 | 2026-10-23 | Genesis |
| 101 | 2026-10-25 | An address in contracts belongs to its first owner ([contracts.md](contracts.md#addresses)) |
| 102 | 2026-10-25 | The same runtime, built with the paths of the build machine remapped |
| 103 | 2026-10-25 | Governance in place of sudo: referenda, conviction votes, the technical committee ([governance.md](governance.md)) |
| 104 | 2026-10-26 | Atomic swaps of TESS against BTC, ETH, USDC and USDT ([swaps.md](swaps.md)): the first upgrade by referendum, #0 on the urgent track, whitelisted by the committee |

## Demo maker

A maker of the testnet sells TESS for testnet4 BTC, so that the market of the wallet always has an
offer: `tesserax-wallet swap make` with `--keep-offer` ([swaps.md](swaps.md#using-it)), started by
`docker/demo-maker.sh`. It offers 1,000 TESS at a time, at 0.00001 BTC each, from 10 TESS.
