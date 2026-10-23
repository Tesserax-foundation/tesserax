# Devnet

The devnet is the public development network of Tesserax, ahead of the testnet: the chain the
explorer indexes and the web wallet connects to while the network is being built. Its tokens
have no value, its sudo key is the development account `//Alice`, and it may be reset.

| | |
|---|---|
| Chain specification | [`chain-specs/devnet.json`](../chain-specs/devnet.json), in raw format |
| Genesis hash | `0x6ad86a190e24dc64fc5203641d9a4ba4f1db45f9f7a7aa38fe950de3837caee7` |
| Bootnode | `/dns/interchange.proxy.rlwy.net/tcp/28385/p2p/QmPUY5BhQfshJqb3vUhZ7utcsVggZXRFhygi4hP4NjcGve` |

The specification holds the genesis state itself, runtime included: a node started with it joins
this chain whatever the version of its own runtime.

## Join it

```bash
tesserax --chain chain-specs/devnet.json --mine --author <your address>
```

The bootnode is in the specification.

In Docker, the specifications are in `/usr/share/tesserax` of the image. Without `--mine`, the
node only follows the chain; with `--stratum`, XMRig mines on it (see [mining.md](mining.md)).

## Hosting

The devnet runs on Railway, project `tesserax`, region `us-west2` next to the Supabase database of
the explorer, in three services.

### `node`

An archive node, which does not mine: it serves the chain to the indexer and is the bootnode of
the network.

- Source: `docker/Dockerfile.devnet`, deployed with the Railway CLI from the repository
  (`railway up --service node --detach`). Railway gives an image build about twenty minutes, less
  than the node takes to compile: the image holds the sources and the toolchain, and the node is
  compiled when the container starts (`docker/devnet-node.sh`), without its development runtime,
  into the volume. A restart on the same sources starts at once; a deploy of new sources compiles
  again, during which the node is down.
- Variable `CARGO_BUILD_JOBS=6`, so that the compilation fits in the memory of the service.
- Volume: mounted on `/data`. The variable `RAILWAY_RUN_UID=0` lets the node write to it, the
  volume belonging to root.
- TCP proxy: public port to the internal port `30333`, the peer-to-peer port. The node announces
  that address (`--public-addr`, from `RAILWAY_TCP_PROXY_DOMAIN` and `RAILWAY_TCP_PROXY_PORT`).
- Start command:

  ```bash
  /bin/sh -c "exec /build/docker/devnet-node.sh --chain /build/chain-specs/devnet.json --base-path /data --state-pruning archive --name tesserax-devnet-railway --listen-addr /ip4/0.0.0.0/tcp/30333 --listen-addr /ip6/::/tcp/30333 ${RAILWAY_TCP_PROXY_DOMAIN:+--public-addr /dns/$RAILWAY_TCP_PROXY_DOMAIN/tcp/$RAILWAY_TCP_PROXY_PORT} --experimental-rpc-endpoint listen-addr=[::]:9944,methods=safe --experimental-rpc-endpoint listen-addr=[::]:9945,methods=safe,rate-limit=600,max-connections=1000,max-subscriptions-per-connection=64,max-batch-request-len=50,max-request-size=10,max-response-size=10 --no-prometheus --no-telemetry --no-mdns"
  ```

- Restart policy: always.

The RPC has two endpoints, each taking IPv4 and IPv6 connections alike (with `--rpc-external`,
Linux gives the IPv6 listener a random port, and the private network of Railway may be IPv6
only):

- `[::]:9944`, without limits, for the indexer;
- `[::]:9945`, rate limited, for the public through the `rpc` service.

Neither has a public domain of Railway: the public reaches the node through the gateway only.

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

Only one indexer may write to a database: stop any other one before starting this one.

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
