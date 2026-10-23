# Devnet

The devnet is the public development network of Tesserax, ahead of the testnet: the chain the
explorer indexes and the web wallet connects to while the network is being built. Its tokens
have no value, its sudo key is the development account `//Alice`, and it may be reset.

| | |
|---|---|
| Chain specification | [`chain-specs/devnet.json`](../chain-specs/devnet.json), in raw format |
| Genesis hash | `0x6ad86a190e24dc64fc5203641d9a4ba4f1db45f9f7a7aa38fe950de3837caee7` |
| Bootnode | `/dns/interchange.proxy.rlwy.net/tcp/28385/p2p/<peer id>` |

The specification holds the genesis state itself, runtime included: a node started with it joins
this chain whatever the version of its own runtime.

## Join it

```bash
tesserax --chain chain-specs/devnet.json --bootnodes <bootnode> --mine --author <your address>
```

In Docker, the specifications are in `/usr/share/tesserax` of the image. Without `--mine`, the
node only follows the chain; with `--stratum`, XMRig mines on it (see [mining.md](mining.md)).

## Hosting

The devnet runs on Railway, project `tesserax`, region `us-west2` next to the Supabase database of
the explorer, in two services.

### `node`

An archive node, which does not mine: it serves the chain to the indexer and is the bootnode of
the network.

- Source: the image `ghcr.io/wayleyy/tesserax:main`, which CI publishes once a push to `main`
  passes. Building the node takes longer than Railway allows a build, so Railway never builds it:
  redeploying the service pulls the latest image.
- Volume: mounted on `/data`. The variable `RAILWAY_RUN_UID=0` lets the node write to it, the
  volume belonging to root.
- TCP proxy: public port to the internal port `30333`, the peer-to-peer port. The node announces
  that address (`--public-addr`, from `RAILWAY_TCP_PROXY_DOMAIN` and `RAILWAY_TCP_PROXY_PORT`).
- Start command:

  ```bash
  /bin/sh -c "exec tesserax --chain /usr/share/tesserax/devnet.json --base-path /data --state-pruning archive --name tesserax-devnet-railway --listen-addr /ip4/0.0.0.0/tcp/30333 --listen-addr /ip6/::/tcp/30333 ${RAILWAY_TCP_PROXY_DOMAIN:+--public-addr /dns/$RAILWAY_TCP_PROXY_DOMAIN/tcp/$RAILWAY_TCP_PROXY_PORT} --rpc-external --rpc-methods safe --rpc-cors all --no-prometheus --no-telemetry --no-mdns"
  ```

- Restart policy: always.

The RPC of the node has no public domain: the indexer reaches it on the private network of the
project. A public RPC needs post-quantum TLS in front of it, which the proxies of Railway do not
offer.

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
