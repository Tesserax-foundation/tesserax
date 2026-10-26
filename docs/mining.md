# Mining

Tesserax blocks are mined on CPUs with RandomTESS, the Tesserax configuration of RandomX v2. A
block pays its miner 0.85 TESS (halving every 7 000 000 blocks) plus all the fees and tips of its
transactions; nothing is burned.

There are two ways to mine:

- the built-in miner of the node (`--mine`), the simplest;
- XMRig with the `rx/tess` algorithm, connected to the stratum server of a node (`--stratum`).
  XMRig is several times faster than the built-in miner on the same CPU.

## RandomTESS

RandomTESS is RandomX v2 (384 instructions per program, the v2 CFROUND, AES and dataset prefetch
changes, and the v2 commitment) with:

| Parameter | RandomX | RandomTESS |
|---|---|---|
| Argon2 salt | `RandomX\x03` | `RandomTESS\x01` |
| `IROR_R` / `IROL_R` frequencies | 8 / 2 | 6 / 4 |
| `FADD_M` / `FSUB_M` frequencies | 5 / 5 | 4 / 6 |

Memory requirements are those of RandomX: 2 GiB of dataset to mine at full speed (256 MiB in light
mode), 256 MiB to verify.

The work input of a block is 43 bytes: the hash of the header without its seal, then an 11-byte
nonce. Miners iterate the last 4 bytes of the nonce, at offset 39 as in Monero; the 7 bytes before
are an extra nonce set by the stratum server for each connection.

The proof-of-work value is the RandomX v2 commitment `Blake2b-256(input || hash)`, where `hash` is
the RandomTESS hash of the input under the key of the block. The seal of a block carries the nonce
and the hash, so a node checks the commitment against the difficulty with one Blake2b before it
runs the virtual machine: an invalid block costs nothing to reject.

The key is the hash of a past block: it changes every 2048 blocks (about 10 hours), 64 blocks
after the key block, so that miners prepare the next dataset in advance.

## Built-in miner

```bash
tesserax --chain <spec> --mine --author <address>
```

`--author` is the account credited with the mined blocks. `--mining-threads N` sets the number of
threads (all cores by default) and `--mining-light` mines with 256 MiB instead of 2 GiB, about ten
times slower.

## XMRig

The node serves block templates to XMRig over stratum:

```bash
tesserax --chain <spec> --stratum --author <address>
```

The server listens on `127.0.0.1:3333`; `--stratum 0.0.0.0:3333` accepts rigs of the local network.
It has no authentication: keep it behind a firewall, on a private network. `--stratum-max-miners`
limits the number of connected rigs (256 by default). `--mine` and `--stratum` can be combined.

Every block mined through the server credits `--author`. The login of a rig is its name (`rig1`),
or the address of the author; the server refuses another address, so that a rig never mines to a
node believing that the rewards go elsewhere.

Download the `rx/tess` miner, a fork of XMRig 6.26.0, from the
[releases of `tesserax-miner`](https://github.com/Tesserax-foundation/tesserax-miner/releases): static binaries
for Linux (x64, arm64), macOS (Apple Silicon, Intel) and Windows (x64), with their SHA-256 in
`SHA256SUMS`. Every binary is checked against the reference RandomTESS implementation, in light and
fast mode, before it is published. On macOS, allow it once with
`xattr -d com.apple.quarantine xmrig`.

Or build it:

```bash
git clone https://github.com/Tesserax-foundation/tesserax-miner.git
cd tesserax-miner && mkdir build && cd build
cmake .. -DWITH_OPENCL=OFF -DWITH_CUDA=OFF
make -j$(nproc)
```

and point it to the node:

```bash
./xmrig -o 127.0.0.1:3333 -u rig1 -a rx/tess
```

The fork has no developer fee: XMRig normally gives 1% of the hashrate to its developers, this
build gives 0% whatever the `donate-level` option says. RandomTESS runs on CPUs only; the GPU
backends of XMRig do not implement RandomX v2 and are disabled for `rx/tess`.

Each rig gets a job whose target is the block difficulty: XMRig submits a share only when it finds
a block. The node logs `Block #N mined by stratum miner rig1` for each of them.

A share found for a block that was mined in the meantime is answered `Stale share`: it cannot make
a block any more. Stale shares stay rare when the node imports blocks quickly; a machine that runs
both the node and XMRig needs about 3 GiB of free memory for them (2 GiB of dataset, 256 MiB of
verification cache, the node itself), or XMRig in light mode (`"randomx": {"mode": "light"}`).

Measured on an Apple M1 (8 GiB), 4 threads: about 600 H/s with XMRig against about 100 H/s with
the built-in miner.

### Protocol

The server speaks the stratum protocol of RandomX pools (JSON-RPC, one message per line):

- `login` (`login`, `pass`, `agent`, `algo`, `rigid`) returns a session id and a job;
- `job` is pushed whenever the block template changes: `blob` (43 bytes, hex), `job_id`,
  `target` (64-bit little-endian, hex), `algo` (`rx/tess`), `height`, `seed_hash` (the RandomTESS
  key);
- `submit` (`id`, `job_id`, `nonce`: 4 bytes hex, `result`: the commitment, `commitment`: the
  RandomTESS hash) returns `OK` when the block is imported;
- `keepalived` keeps the connection open; an idle connection closes after 5 minutes.

Every job also carries `difficulty`, the exact block difficulty as a decimal string.

A mining pool logs in with `"mode": "pool"`. Its jobs then carry `reserved_offset` (35) and
`reserved_size` (4): the node fixes the first 3 bytes of the extra nonce, the pool sets the next 4
for each of its rigs, and submits the blocks they find with that `extra_nonce` (4 bytes, hex). The
extra nonces of pools and of rigs never overlap. See [the mining pool](pool.md).

`tesserax-pool` is a mining pool built on this mode: PPLNS rewards, verified shares and automatic
payouts. See [the mining pool](pool.md).
