# Testnet

The testnet is the public network of Tesserax ahead of the mainnet: the chain the explorer, the
public RPC and the web wallet follow. It has the tokenomics of the mainnet: the same emission, the
same allocations to the team and the foundation (test accounts) and the same fees. Its TESS are
test TESS: they have no value, and the testnet may be reset.

| | |
|---|---|
| Chain | `tesserax --chain testnet`, [`chain-specs/testnet.json`](../chain-specs/testnet.json) |
| Genesis hash | `0xc2f9cb03897c5ad30fff2f93debaa8b4a6e08f37a12270e107c71eb1ad39e5d1` |
| Bootnode | `/dns/interchange.proxy.rlwy.net/tcp/28385/p2p/QmUAEp2bK2V3ADJkZym99dAEZZBN6WccwLecjfAFzpDWGo` |
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

## Hosting

The testnet runs on Railway, project `tesserax`, region `us-west2` next to the Supabase database of
the explorer, in seven services.

### `node`

An archive node: the bootnode of the network, the node the indexer and the public RPC read, and
the node of the pool, which mines through its stratum server.

- Source: `docker/Dockerfile.from-source`, deployed with the Railway CLI from the repository
  (`railway up --service node --detach`). Railway gives an image build about twenty minutes, less
  than the node takes to compile: the image holds the sources and the toolchain, and the node is
  compiled when the container starts (`docker/from-source.sh`), without its development runtime,
  into the volume. A restart on the same sources starts at once; a deploy of new sources compiles
  again, during which the node is down.
- Variables: `CARGO_BUILD_JOBS=6`, so that the compilation fits in the memory of the service;
  `POOL_AUTHOR`, the address of the pool account, which the blocks mined through the stratum
  server pay; `RAILWAY_RUN_UID=0`, so that the node writes to its volume.
- Volume: mounted on `/data`.
- TCP proxy: `interchange.proxy.rlwy.net:28385` to the internal port `30333`, the peer-to-peer
  port. The node announces that address (`--public-addr`, from `RAILWAY_TCP_PROXY_DOMAIN` and
  `RAILWAY_TCP_PROXY_PORT`).
- Start command:

  ```bash
  /bin/sh -c "exec /build/docker/from-source.sh --chain testnet --base-path /data --state-pruning archive --name tesserax-testnet-railway --listen-addr /ip4/0.0.0.0/tcp/30333 --listen-addr /ip6/::/tcp/30333 ${RAILWAY_TCP_PROXY_DOMAIN:+--public-addr /dns/$RAILWAY_TCP_PROXY_DOMAIN/tcp/$RAILWAY_TCP_PROXY_PORT} --experimental-rpc-endpoint listen-addr=[::]:9944,methods=safe --experimental-rpc-endpoint listen-addr=[::]:9945,methods=safe,rate-limit=600,max-connections=1000,max-subscriptions-per-connection=64,max-batch-request-len=50,max-request-size=10,max-response-size=10 --stratum [::]:3334 --stratum-max-miners 8 --author $POOL_AUTHOR --no-prometheus --no-telemetry --no-mdns"
  ```

- Restart policy: always.

The RPC has two endpoints, each taking IPv4 and IPv6 connections alike (with `--rpc-external`,
Linux gives the IPv6 listener a random port, and the private network of Railway may be IPv6
only):

- `[::]:9944`, without limits, for the indexer and the faucet;
- `[::]:9945`, rate limited, for the public through the `rpc` service.

Neither has a public domain of Railway: the public reaches the node through the gateway only.
The stratum server, `[::]:3334`, has none either: only the pool reaches it, on the private network.

### `faucet`

`tesserax-faucet` (`faucet/`): its page and its API. Its key is generated on its first start and
kept in its volume (`faucet.phrase`); it never leaves the service.

- Source: `docker/Dockerfile.from-source`, with `PACKAGE=tesserax-faucet` and
  `BINARY=tesserax-faucet`, compiled when it starts like the node.
- Variables: `NODE_URL=ws://${{node.RAILWAY_PRIVATE_DOMAIN}}:9944`, `LISTEN=[::]:8080`,
  `PORT=8080`, `EXPLORER=https://explorer.tesserax.app`, `RAILWAY_RUN_UID=0`,
  `CARGO_BUILD_JOBS=6`. `AMOUNT`, `IP_CLAIMS` and `DAILY_CAP` change its limits (10 TESS per
  claim, 3 claims per IP address and 2,000 TESS every 24 hours by default).
- Volume: mounted on `/data`, its key and the claims of the last 24 hours.
- Domain: `faucet.tesserax.app`, a CNAME on Vercel DNS to the domain Railway gives it.
- Start command: `/build/docker/from-source.sh`.

### `indexer`

The indexer of the explorer ([explorer/README.md](../explorer/README.md)).

- Source: `explorer/indexer/Dockerfile`, built from the `explorer` directory and deployed with
  the Railway CLI from the repository:

  ```bash
  railway up explorer --path-as-root --service indexer --detach
  ```

- Variables:

  | Variable | Value |
  |---|---|
  | `NODE_URL` | `ws://${{node.RAILWAY_PRIVATE_DOMAIN}}:9944` |
  | `DATABASE_URL` | connection string of the Supabase project, session pooler (secret) |
  | `BATCH` | `16` |
  | `LOG_LEVEL` | `info` |

- Restart policy: always. It resumes where it stopped.

Only one indexer may write to a database: stop any other one before starting this one. After a
reset of the chain, empty the tables of the database before starting it again.

### `rpc`

The public RPC ([rpc.md](rpc.md)): `tesserax-rpc-gateway`, which terminates post-quantum TLS
itself and relays to the endpoint `9945` of the node.

- Source: `rpc-gateway/Dockerfile`, deployed with the Railway CLI from the repository
  (`railway up --service rpc --detach`).
- Volume: mounted on `/data`, the ACME account and the certificate; `RAILWAY_RUN_UID=0`.
- TCP proxy: `maglev.proxy.rlwy.net:21207` to the internal port `8443`. The DNS record
  `rpc.tesserax.app` is a CNAME of `maglev.proxy.rlwy.net`, on Vercel DNS.
- Variables:

  | Variable | Value |
  |---|---|
  | `UPSTREAM` | `${{node.RAILWAY_PRIVATE_DOMAIN}}:9945` |
  | `DOMAIN` | `rpc.tesserax.app` |
  | `DNS_ZONE` | `tesserax.app` |
  | `VERCEL_TEAM` | the team owning the zone on Vercel |
  | `VERCEL_TOKEN` | a token of the Vercel API, for the DNS-01 challenges (secret) |

- Restart policy: always.

### `pool`

`tesserax-pool` (`pool/`, [pool.md](pool.md)): the stratum server of the rigs, the dashboard and
the API. Its account is generated on its first start and kept in its volume (`pool.key`), with
the database of shares, balances and payouts (`pool.sqlite`): back both up.

- Source: `docker/Dockerfile.from-source`, with `PACKAGE=tesserax-pool` and
  `BINARY=tesserax-pool`, compiled when it starts like the node, and deployed with
  `railway up --service pool --detach`.
- Variables: `NODE_HOST=${{node.RAILWAY_PRIVATE_DOMAIN}}`,
  `STRATUM_URL=stratum.tesserax.app:25202`, `PORT=8080`, `RUST_LOG=info`, `RAILWAY_RUN_UID=0`,
  `CARGO_BUILD_JOBS=6`.
- Volume: mounted on `/data`.
- TCP proxy: `trolley.proxy.rlwy.net:25202` to the internal port `3333`, the stratum port. The
  DNS record `stratum.tesserax.app` is a CNAME of `trolley.proxy.rlwy.net`, on Vercel DNS. The
  proxy hides the IP addresses of the rigs, hence `--behind-proxy`.
- Domain: `pool.tesserax.app`, the dashboard, a CNAME on Vercel DNS to the domain Railway gives
  it, on the internal port `8080`.
- Start command:

  ```bash
  /bin/sh -c "exec /build/docker/from-source.sh --key-file /data/pool.key --create-key --db /data/pool.sqlite --node-stratum $NODE_HOST:3334 --node-rpc ws://$NODE_HOST:9944 --listen [::]:3333 --api [::]:8080 --behind-proxy --stratum-url $STRATUM_URL --network Testnet --name \"Tesserax Testnet Pool\" --explorer https://explorer.tesserax.app --min-payout 1 --payout-interval 30"
  ```

- Restart policy: always.

### `demo-maker`

A maker selling TESS for testnet4 BTC, so that the market of the wallet always has an offer:
`tesserax-wallet swap make` with `--keep-offer` ([swaps.md](swaps.md#using-it)). It mines
nothing. Its account, the password of its keystore and its Bitcoin key are created on its first
start and kept in its volume; they never leave the service. It offers the TESS sent to its
account, 1,000 at a time, at 0.00001 BTC each, from 10 TESS, and collects the BTC paid on its
Bitcoin key.

- Source: `docker/Dockerfile.from-source`, compiled when it starts like the node, and deployed
  with `railway up --service demo-maker --detach`.
- Variables: `NODE_URL=ws://${{node.RAILWAY_PRIVATE_DOMAIN}}:9944`, `RAILWAY_RUN_UID=0`,
  `CARGO_BUILD_JOBS=6`; `KEEP_OFFER` changes its offers (`bitcoin:btc:0.00001:1000:10` by
  default).
- Volume: mounted on `/data`, its keystore (`wallet/`), its password (`maker.password`), its
  Bitcoin key (`btc.wif`) and what it has read (`wallet/swaps/`): back them up.
- Start command: `/build/docker/demo-maker.sh`. Its log starts with the address of its account,
  to send TESS to.
- Restart policy: always.

### `rig`

A rig of the pool with one core, so that blocks never stop: the Tesserax build of XMRig in light
mode, paying the faucet.

- Source: `docker/Dockerfile.xmrig`, the release of the miner checked against its SHA-256,
  deployed with `railway up --service rig --detach`.
- Variables: `POOL_HOST=${{pool.RAILWAY_PRIVATE_DOMAIN}}`, `PAYOUT_ADDRESS`, the address of the
  faucet.
- Start command:

  ```bash
  /bin/sh -c "exec /opt/xmrig/xmrig --algo rx/tess --no-color --url $POOL_HOST:3333 --user $PAYOUT_ADDRESS.railway --pass x --threads 1 --randomx-mode=light --dns-ipv6 --print-time 300"
  ```

- Restart policy: always.
