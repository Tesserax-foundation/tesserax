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
- Writes a batch of blocks in one transaction, its statements pipelined: catching up costs a
  round trip to the database per batch, not per statement. Byte strings over 512 bytes (shielded
  proofs) are stored as their length and Blake2 hash.

The node must keep the state of every block it indexes: run it as an archive node
(`--state-pruning archive`) to index a chain from genesis.

## Set up

1. Create a project on [Supabase](https://supabase.com).
2. Apply the schema, with the Supabase CLI:

   ```bash
   supabase login
   supabase link --project-ref <project ref>
   supabase db push
   ```

   or by pasting `supabase/migrations/*.sql` in the SQL editor of the project.
3. Copy `indexer/.env.example` to `indexer/.env` and set `DATABASE_URL` to the connection string
   of the project (Project Settings > Database, session pooler) and `NODE_URL` to the node.
   The connection string writes to the database: keep it secret.
4. Copy `web/.env.example` to `web/.env` and set `VITE_SUPABASE_URL` and
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

## Deploy

- Web application: `npm run build -w web` produces `web/dist`, a static site. `web/vercel.json`
  and `web/public/_redirects` send every path to `index.html` on Vercel and Netlify.
- Indexer: one process per database, next to the node, restarted on failure (systemd, Docker).
  It resumes where it stopped. The indexer of the devnet runs on Railway next to its node: see
  [docs/devnet.md](../docs/devnet.md).

## Tests

```bash
npm test -w indexer                                          # schema and database writes, on PGlite
TESSERAX_URL=ws://127.0.0.1:9944 npm test -w indexer         # also indexes a development chain
```

The tests run the migrations on PGlite, Postgres in WebAssembly, served over the Postgres protocol:
no database to install. The development chain test indexes the whole chain, checks every block,
account, agent, channel and task against the node, then follows a reorganization.
