# Mining pool

`tesserax-pool` runs a public mining pool: rigs running the `rx/tess` miner (the Tesserax build of
XMRig) connect with the address their rewards go to, the pool mines to its own account through its
node, and pays each miner its part of the blocks found.

```text
rigs (XMRig, rx/tess) ──stratum──▶ tesserax-pool ──stratum, pool mode──▶ node (--author = pool)
                                        │  ▲
                                SQLite  │  └──RPC── node: finality of the blocks, rewards, payouts
                                        └──HTTP── dashboard and API
```

## How it works

- **Work**: the node fixes the first 3 bytes of the 7-byte extra nonce for the pool, the pool sets
  the next 4 for each rig (`reserved_offset` 35, `reserved_size` 4 in the jobs of the node). Every
  rig hashes inputs of its own; one connection to the node serves billions of rigs.
- **Shares**: each rig gets a share difficulty that follows its hashrate, aiming at one share every
  15 seconds (`--share-time`), or a fixed one when it logs in with `address+difficulty`. The pool
  checks every share in full: the proof-of-work value against the share difficulty, then the
  RandomTESS hash, recomputed in light mode. A rig can make up hashes whose value passes the share
  difficulty for free, so a wrong hash bans its IP address for `--ban-minutes`. Duplicate and stale
  shares are refused.
- **Blocks**: a share that also meets the block difficulty goes to the node, which verifies it again
  and imports the block. The node credits its blocks to its `--author`, the pool account; it
  refuses the pool if that is not the address of the pool key, so rewards cannot go elsewhere.
- **PPLNS**: when a block is found, the shares of the last `--pplns-factor` times its difficulty
  (2 by default) split its reward, in proportion to their difficulty. The split is recorded at
  once, and credited when the block is `--maturity` blocks deep (60, the depth past which the chain
  never reorganizes). A block that left the chain pays nothing. The reward is what the block paid
  the pool: the mining reward and all the fees of its transactions, read from its events.
- **Fee**: `--fee` percent of each block (1 by default) stays in the pool account. It pays the fees
  of the payouts; rounding remainders go to it too.
- **Payouts**: every `--payout-interval` minutes, the pool pays each balance of at least
  `--min-payout` TESS with a transfer from the pool account. A payout is written to the database
  before it is sent: if the pool stops while sending, the payout is marked `unknown` and stays off
  the balance, so that it is never paid twice; the operator checks it on the chain.

## Running a pool

The node mines through its stratum server, on a private address, with the pool account as author:

```bash
tesserax --chain <spec> --stratum 127.0.0.1:3334 --stratum-max-miners 4 --author <pool address>
```

It can be a full node: the pool reads the chain at the heights of its own blocks only.

The pool needs the secret phrase of the pool account, to pay. Keep it in a file only its owner can
read:

```bash
umask 077 && echo "<secret phrase>" > pool.key
tesserax-pool --key-file pool.key \
  --node-stratum 127.0.0.1:3334 --node-rpc ws://127.0.0.1:9944 \
  --listen 0.0.0.0:3333 --api 0.0.0.0:8080 \
  --stratum-url pool.example.com:3333 --explorer https://explorer.tesserax.app \
  --fee 1 --min-payout 1
```

or pass it in the environment variable `TESSERAX_POOL_KEY`, which the pool clears once read.
`tesserax-pool --help` lists every option. Logs follow `RUST_LOG` (`info` by default).

- **Memory**: the verification of shares needs one light RandomTESS cache per key, 256 MiB, two
  around a key change. `--verify-threads` (2 by default) sets how many shares are verified at
  once: each costs about one hash, a few milliseconds.
- **Database**: `--db` (`tesserax-pool.sqlite`), in WAL mode. Back it up: it holds the balances.
  Shares older than a day are pruned once no PPLNS window can use them.
- **Network**: open the stratum port to the rigs, put the dashboard behind a TLS proxy. The node
  stratum and RPC stay private.

## Mining on a pool

```bash
./xmrig -a rx/tess -o pool.example.com:3333 -u <your address>.<rig name> -p x
```

- `-u` is your Tesserax address (`te…`), where your rewards go, then `.` and a name for the rig.
- `-u <address>.<rig>+50000` fixes the share difficulty instead of letting the pool adjust it.
- The dashboard of the pool shows your hashrate, each rig, your balance, what is not final yet,
  and your payouts.

## API

All read-only, JSON, open to other sites.

| Path | Content |
|---|---|
| `GET /api/stats` | Pool hashrate, miners, rigs, blocks found, round effort, network height and difficulty, terms of the pool |
| `GET /api/blocks?limit=50` | Blocks found: height, hash, finder, effort, status (`pending`, `confirmed`, `orphaned`), reward |
| `GET /api/payments?limit=50` | Payouts: address, amount, status, transaction |
| `GET /api/miners/{address}` | Hashrate, rigs, balance, immature, paid and payouts of a miner |

Amounts are in planck (10^-12 TESS), as decimal strings.
