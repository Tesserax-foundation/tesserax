# Running a node

A node follows the chain, checks every block and transaction, relays them to its peers and serves
the RPC that wallets and applications use. Anyone can run one, on the testnet or the mainnet; it
needs no stake and no permission.

## Hardware

| Use | CPU | Memory | Disk | Network |
|---|---|---|---|---|
| **Full node**: follow the chain, relay, serve one's own wallet | 2 cores | 4 GB | 100 GB SSD | 10 Mbit/s |
| **Public node**: bootnode, node others connect to | 4 cores | 8 GB | 200 GB NVMe SSD | 100 Mbit/s, port 30333 open |
| **Archive node**: public RPC, explorer indexer, mining pool | 4–8 cores | 16 GB | 500 GB+ NVMe SSD | 100 Mbit/s or more |
| **Mining node**, fast mode | every core it can spare | 8 GB: 2.5 GB more for the dataset | as a full node | as a full node |

The same figures apply to the testnet and the mainnet.

- **CPU.** A block holds up to 2 seconds of computation every 18 seconds, and the node checks each
  one: its RandomTESS proof of work in light mode, every ML-DSA-87 signature (about 1 ms each) and
  every private-transfer proof (about 12 ms). Two cores keep up; four make the first
  synchronization faster.
- **Memory.** The node keeps two RandomTESS caches of 256 MB, one per key around a key change, next
  to its database cache. Mining in fast mode builds a dataset of 2,080 MB more.
- **Disk.** An SSD is required: the database reads and writes small records at random, which a hard
  disk cannot keep up with. A full node keeps every block but only recent state; an archive node
  keeps every state, as RPC services need. Blocks can carry up to 5 MiB, so the chain may grow fast
  under heavy use: watch the free space, and start with the sizes above.
- **Network.** Even full blocks stay below 3 Mbit/s. Opening TCP port 30333 lets other nodes
  connect and helps the network; a node behind a firewall still syncs through its outgoing
  connections.
- **Clock.** The clock must be synchronized (NTP, on by default on Linux and macOS): a block
  stamped more than 30 seconds in the future is refused.
- **System.** Linux x86-64 or ARM64 (Ubuntu 22.04 or 24.04 recommended) or macOS. A container image
  is published for each release.

Any VPS fits a full node: running a node is allowed by nearly every host. Mining is not: most hosts
forbid it in their terms, so check them before mining on a rented server ([mining.md](mining.md)).

## Starting

Build the node ([README](../README.md#building)) or take it from a release, then:

```bash
tesserax --chain testnet --base-path /var/lib/tesserax                          # full node
tesserax --chain testnet --base-path /var/lib/tesserax --state-pruning archive  # archive node
```

On the mainnet, `--chain mainnet` once it is launched ([mainnet.md](mainnet.md)). The chain
specification is built into the node with its bootnodes: the node finds its peers by itself.

| Port | Use | Expose |
|---|---|---|
| 30333 | Peer to peer, post-quantum TLS | Yes, to help the network |
| 9944 | JSON-RPC and WebSocket | Only on the machine or a private network; publish it behind a gateway ([rpc.md](rpc.md)) |
| 3333 | Stratum, with `--stratum` | Only to your miners |

The node identity is an ML-DSA-87 key kept in `<base path>/chains/<chain>/network/secret_ml_dsa`:
keep it to keep the same peer id across restarts, which bootnodes need.

### As a service

On Linux, a systemd unit keeps the node running and restarts it after a failure or a reboot:

```ini
# /etc/systemd/system/tesserax.service
[Unit]
Description=Tesserax node
After=network-online.target
Wants=network-online.target

[Service]
User=tesserax
ExecStart=/usr/local/bin/tesserax --chain testnet --base-path /var/lib/tesserax --name my-node
Restart=always
RestartSec=10
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

```bash
sudo useradd --system --home /var/lib/tesserax --create-home tesserax
sudo systemctl enable --now tesserax
journalctl -u tesserax -f
```

To mine as well, add `--mine --author <your address>`, or serve XMRig with `--stratum`
([mining.md](mining.md)).
