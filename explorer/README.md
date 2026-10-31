# Tesserax Explorer

The block explorer of Tesserax: blocks, transactions, transfers, accounts, miners, the agent
registry, the task market, payment channels, emission and the shielded pool.

```
Tesserax node ──RPC──▶ indexer ──Postgres──▶ Supabase ──REST + realtime──▶ web application
```

| Directory | Contents |
|---|---|
| `supabase/migrations/` | Database schema, read-only access for everyone, and the functions the web application calls |
| `indexer/` | Follows the best chain of a node into the database (TypeScript, Node.js 22) |
| `web/` | The web application (React, TypeScript, Vite, Tailwind CSS) |

The chain cannot answer "every transaction of this address" or "the latest transfers": the
indexer keeps that index in Supabase. It is the only centralized part, and anyone can rebuild it
from a node: it holds nothing that is not on chain.

## Indexer

- Decodes every block with the metadata of its runtime: extrinsics (ML-DSA-87 signed), events,
  the author and the seal (nonce and RandomTESS hash) of the proof of work, the difficulty, the
  reward and the fees, and dispatch errors by name.
- Follows reorganizations: blocks that leave the best chain stay in the database as orphaned
  blocks, without their transactions, and the accounts, agents, delegations, channels and tasks
  they touched are read again from the chain. Blocks 60 deep are marked final.
- Keeps accounts, agents, delegations, channels and tasks as their state on chain, read again
  after each block that touches them, and the head, supply, emission, shielded pool, constants
  and genesis allocations in `chain_state`.
- Keeps the open offers and bids of atomic swaps (`swap_offers`, `swap_bids`), read again after
  each block with a swap event; the trades and the sales into bids come from the events of the
  swaps pallet (`swap_trades`, `swap_sales`), so a reorganization drops them with their blocks.
- Writes a batch of blocks in one transaction, its statements pipelined: catching up costs a
  round trip to the database per batch, not per statement. Byte strings over 512 bytes (shielded
  proofs) are stored as their length and Blake2 hash.

The node must keep the state of every block it indexes: run it as an archive node
(`--state-pruning archive`) to index a chain from genesis.

## Set up

1. Create a project on [Supabase](https://supabase.com).
2. Give the indexer its password: write `supabase/roles.sql` (git-ignored) with a random one.

   ```sql
   do $$ begin
     if not exists (select from pg_roles where rolname = 'explorer_indexer') then
       create role explorer_indexer login noinherit;
     end if;
   end $$;
   alter role explorer_indexer with login password '<openssl rand -hex 32>';
   ```

3. Apply the schema and the role, with the Supabase CLI:

   ```bash
   supabase login
   supabase link --project-ref <project ref>
   supabase db push --include-roles
   ```

   The indexer writes as `explorer_indexer`, which can change the rows of the explorer's tables
   and nothing else: not the schema, not the other schemas of the project.
4. Copy `indexer/.env.example` to `indexer/.env` and set `DATABASE_URL` to the connection string
   of that role through the session pooler (Project Settings > Database), and `NODE_URL` to the
   node. The connection string writes to the database: keep it secret.
5. Copy `web/.env.example` to `web/.env` and set `VITE_SUPABASE_URL` and
   `VITE_SUPABASE_ANON_KEY` (Project Settings > API). The anon key is public: row level security
   limits it to reading.

## Run

```bash
npm install
npm run indexer          # reads indexer/.env
npm run dev              # web application on http://localhost:5173
```

The indexer also runs with Docker, its image built from this directory for the lockfile:

```bash
docker build -f indexer/Dockerfile -t tesserax-indexer .
docker run --env-file indexer/.env tesserax-indexer
```

## Market API

The web deployment serves the market of TESS (atomic swaps against BTC, ETH, USDC and USDT, see
[docs/swaps.md](../docs/swaps.md)) in the formats price aggregators read, from the database
functions of `supabase/migrations/*_market.sql` and `*_bids.sql`. Answers are public and cached
30 seconds.

Offers sell TESS and bids buy them. A claimed trade of an offer is a `buy`, its taker bought TESS;
a claimed sale into a bid is a `sell`, its seller sold TESS. Sales have the trade ids from 10^15
on (10^15 + the sale), apart from the ids of trades.

| Path | Answer |
|---|---|
| `/api/coingecko/pairs` | Pairs: `TESS_BTC`, `TESS_ETH`, `TESS_USDC`, `TESS_USDT` once they have an offer, a bid or a trade |
| `/api/coingecko/tickers` | Last price, 24-hour volumes, high and low of the trades and sales claimed, best bid of the open bids, best ask of the open offers |
| `/api/coingecko/orderbook?ticker_id=TESS_USDC&depth=100` | The open bids (TESS wanted) and offers (TESS available) by price, `depth` levels in all, half on each side |
| `/api/coingecko/historical_trades?ticker_id=TESS_USDC&limit=500&start_time=&end_time=&type=` | Trades and sales claimed, the latest first: buys and sells |
| `/api/supply` | Total, circulating and maximum supply of TESS, as JSON |
| `/api/supply/total`, `/api/supply/circulating`, `/api/supply/max` | One of them as a plain number |

The circulating supply is the issuance without what the genesis allocations still vest.

## Deploy

- Web application: `npm run build -w web` produces `web/dist`, a static site. `web/vercel.json`
  and `web/public/_redirects` send every path to `index.html` on Vercel and Netlify. On Vercel,
  `web/api/` serves the market API, with the variables of the build (`VITE_SUPABASE_URL`,
  `VITE_SUPABASE_ANON_KEY`).
- Indexer: one process per database, next to the node, restarted on failure (systemd, Docker).
  It resumes where it stopped.

## Tests

```bash
npm test -w indexer                                          # schema and database writes, on PGlite
TESSERAX_URL=ws://127.0.0.1:9944 npm test -w indexer         # also indexes a development chain
```

The tests run the migrations on PGlite, Postgres in WebAssembly, served over the Postgres protocol:
no database to install. The development chain test indexes the whole chain, checks every block,
account, agent, channel and task against the node, then follows a reorganization.
