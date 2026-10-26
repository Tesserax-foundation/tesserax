---
title: "4. Consensus"
description: "RandomTESS proof of work, the seal, the key schedule, the difficulty adjustment, fork choice, finality and mining."
---

# 4. Consensus

Tesserax reaches agreement with proof of work. Anyone may produce a block by finding a nonce whose
hash meets the current difficulty; nodes follow the chain with the greatest cumulative difficulty.
Proof of work needs no identity, no stake and no signature from block producers — the property
that keeps the consensus itself free of any classical cryptographic assumption.

## 4.1 RandomTESS

The hash function is **RandomTESS**, a configuration of RandomX v2 (`crypto/randomtess/`).
RandomX executes, for every hash, randomly generated programs on a virtual machine designed after
general-purpose CPUs — integer and floating-point arithmetic, branches, and random accesses to a
2 MiB scratchpad and a 2 GiB dataset. A dedicated chip would have to implement most of a CPU and
its memory hierarchy, which keeps commodity processors competitive and mining open to anyone.

RandomTESS is the RandomX reference implementation with a different configuration, so that its
hashes are incompatible with every other RandomX chain: hash power rented or redirected from
another network cannot be pointed at Tesserax without changing software, and no existing
optimized implementation applies as-is.

| Constant | RandomX v2 | RandomTESS |
|---|---|---|
| `RANDOMX_ARGON_SALT` | `"RandomX\x03"` | `"RandomTESS\x01"` |
| `RANDOMX_FREQ_IROR_R` | 8 | 6 |
| `RANDOMX_FREQ_IROL_R` | 2 | 4 |
| `RANDOMX_FREQ_FADD_M` | 5 | 4 |
| `RANDOMX_FREQ_FSUB_M` | 5 | 6 |

The new salt changes the Argon2 cache derived from every key, and with it the dataset and every
hash. The instruction frequencies are rebalanced only between functionally equivalent pairs
(rotations right and left, floating-point addition and subtraction from memory), a change the
RandomX configuration guide lists as safe: the work per hash and the resistance to specialized
hardware are those of RandomX. All other parameters are unchanged:

| Parameter | Value |
|---|---|
| Argon2 cache | 256 MiB, 3 iterations, 1 lane |
| Dataset | 2,181,038,016 bytes (2,080 MiB) |
| Program size (v2) | 384 instructions |
| Program iterations | 2,048 |
| Programs per hash | 8 |
| Scratchpad | L1 16 KiB, L2 256 KiB, L3 2 MiB |
| Superscalar latency | 170 cycles |

Two modes give identical results:

- **Fast mode** builds the full 2 GiB dataset and is used for mining, about ten times faster.
- **Light mode** keeps only the 256 MiB cache and computes dataset items on demand. Nodes verify
  blocks in light mode.

## 4.2 Work input and seal

The miner hashes a 43-byte work input:

$$
\text{input} = \text{pre\_hash} \;\Vert\; \text{nonce}, \qquad |\text{pre\_hash}| = 32,\ |\text{nonce}| = 11
$$

The pre-hash is the hash of the block header without its seal. The header includes a pre-runtime
digest carrying the miner's account, so the proof of work commits to the miner: changing the
recipient of the reward invalidates the work. The 11-byte nonce is split into a 7-byte extra nonce,
which the stratum server assigns per connection so that rigs never search the same space, and the
4 bytes XMRig iterates.

The value compared with the target is the RandomX commitment, a BLAKE2b-256 hash binding the input
to its RandomTESS hash:

$$
h = \text{RandomTESS}_K(\text{input}), \qquad v = \text{BLAKE2b-256}(\text{input} \,\Vert\, h)
$$

A block of difficulty $D$ is valid when $v$, read as a little-endian 256-bit integer, satisfies

$$
v \cdot D < 2^{256} \quad\Longleftrightarrow\quad v \le \left\lfloor \frac{2^{256} - 1}{D} \right\rfloor .
$$

$D$ is therefore the expected number of hashes needed to find a block. The seal appended to the
header is the pair $(\text{nonce}, h)$, 43 bytes.

Verification is ordered from cheapest to most expensive: decode the seal; check that the
commitment of $(\text{input}, h)$ meets the difficulty, one BLAKE2b hash that rejects most invalid
seals before any RandomX work; resolve the key; recompute $h$ in light mode and compare. Nodes keep
two light virtual machines, one per key that can be live around a key change.

## 4.3 Key schedule

The RandomTESS key $K$ changes every 2,048 blocks (about 10 hours) and is the hash of a block buried
64 blocks deep. For a block at height $n$:

$$
\text{seed}(n) = \begin{cases} 0 & n \le 2112 \\ (n - 65)\ \&\ \lnot(2048 - 1) & n > 2112 \end{cases}
\qquad K_n = \text{hash}(\text{block}_{\text{seed}(n)})
$$

The genesis hash is the key up to height 2,112; the first change happens at height 2,113, which uses
the hash of block 2,048. Changing keys forces miners to rebuild their dataset regularly, which
prevents precomputation, and the 64-block lag lets every miner prepare the next dataset before it
is needed. Because the key is a block hash, it cannot be known before that block exists.

## 4.4 Difficulty adjustment

The difficulty is recomputed by the runtime after every block with a linearly weighted moving
average (LWMA) over the last $k \le 120$ solve times, a window of about 36 minutes. Recent blocks
weigh more, which lets the difficulty follow sudden changes of hash power quickly while staying
stable under the natural variance of proof of work.

Let $(t_0, D_0), \dots, (t_k, D_k)$ be the timestamps and difficulties of the last $k+1$ blocks,
oldest first, and $T$ = 18,000 ms. Each solve time is clamped to $[1\ \text{ms},\ 6T]$:

$$
s_i = \min\big(\max(t_i - t_{i-1},\ 1),\ 6T\big), \qquad i = 1, \dots, k
$$

$$
W = \max\!\left( \sum_{i=1}^{k} i \cdot s_i,\ \ \frac{T\,k(k+1)}{20},\ 1 \right)
$$

$$
D_{\text{next}} = \max\!\left( \frac{T\,(k+1)}{2W} \sum_{i=1}^{k} D_i,\ \ D_{\min} \right),
\qquad D_{\min} = 1{,}000
$$

Equivalently, $D_{\text{next}} = \bar{D} \cdot \frac{T \cdot k(k+1)/2}{\sum i\, s_i}$: the average
difficulty, scaled by the ratio between the weighted target and the weighted observed solve times.

- The **upper clamp** of $6T$ on a solve time bounds the effect of a single slow block, or of a
  timestamp set far in the future, on the next difficulty.
- The **floor** on $W$, a tenth of its expected value, bounds any increase to 10 times the average
  difficulty of the window, whatever the timestamps.
- **Timestamps** must be at least 1 second after the parent's and at most 30 seconds ahead of the
  importing node's clock, so a miner cannot make time run backwards or far ahead.

| Network | Initial difficulty | Rationale |
|---|---|---|
| Development | 2,000 | Instant blocks on one machine |
| Testnet | 1,000 | One CPU core mining in light mode |
| Mainnet | 1,000,000 (to be set at launch) | About 55 kH/s at 18 s per block |

The mainnet value matters for the first hour only: too low, and the first miners find blocks in
seconds and take a large share of the early emission until the difficulty catches up; too high,
and the first blocks are slow. It is set just before launch to the expected hash rate multiplied
by 18, measured on the testnet.

## 4.5 Fork choice and finality

Each node keeps, for every block, the cumulative difficulty of its chain, and makes a block the
new best when its cumulative difficulty is strictly greater than that of the current best; ties
keep the current best. This is the heaviest-chain rule: it follows the chain that represents the
most work, not the most blocks.

Proof of work only gives probabilistic finality: a deeper reorganization is always possible with
enough hash power. Tesserax bounds it. On every new best block at height $n$, the node finalizes
the block at height $n - 60$, about 18 minutes earlier. The client then refuses any chain that
does not contain its finalized blocks, whatever the work behind it.

$$
\text{depth}_{\text{final}} = 60 \text{ blocks} \approx 60 \times 18\ \text{s} = 18\ \text{minutes}
$$

This turns a long-range reorganization — the attack where a majority miner secretly builds a
longer chain to reverse payments made hours ago — into something no honest node will follow. The
cost is liveness under a partition: a node isolated from the network for more than 60 blocks may
finalize a minority chain, and must then be resynchronized by its operator. Exchanges and services
should treat a payment as settled once its block is final; wallets and the explorer show that
state.

The probability that an attacker with a fraction $q$ of the hash power overtakes the honest chain
from $z$ blocks behind, in the model of the Bitcoin paper, is

$$
P(q, z) = 1 - \sum_{k=0}^{z} \frac{\lambda^k e^{-\lambda}}{k!} \left( 1 - \left(\tfrac{q}{p}\right)^{z-k} \right),
\qquad \lambda = z \frac{q}{p},\ p = 1 - q .
$$

| Attacker share $q$ | 0.10 | 0.30 | 0.40 | 0.45 |
|---|---|---|---|---|
| $P(q, 60)$ | $< 10^{-16}$ | $\approx 4 \times 10^{-8}$ | $\approx 0.8\%$ | $\approx 23\%$ |

Finality at 60 blocks therefore makes reversal negligible for attackers below a third of the hash
power, and caps what a majority attacker can reverse to the last 18 minutes.

## 4.6 Block production and mining

A block template is built by the node every 10 seconds, or when a new best block arrives, with up
to 2 seconds to select and execute transactions. Three ways to mine exist:

**Built-in miner.** The node mines with its own threads (`--mine`, `--mining-threads`), in fast or
light mode. Each thread starts from a random nonce prefix and tries 32 hashes between template
checks.

**Stratum server.** The node serves block templates to XMRig and compatible miners over the
stratum protocol (JSON-RPC over TCP, one message per line; methods `login`, `submit` and
`keepalived`; `job` notifications). Each job carries the 43-byte blob, a 64-bit target derived
from the block difficulty, the algorithm `rx/tess`, the height, the seed hash and the exact
difficulty. A share submitted to the node is a block: the node checks its commitment and imports
it.

**Mining pool.** Small miners rarely find blocks alone. `tesserax-pool` connects to the node's
stratum in pool mode, where the node leaves 4 bytes of the extra nonce to the pool, and serves rigs
with shares of lower difficulty, adjusted per rig to about one every 15 seconds. Every share is
verified by recomputing its RandomTESS hash. Rewards are shared by PPLNS — pay per last $N$ shares:
when the pool finds a block, the shares submitted just before it, up to a total difficulty of
twice the block's, divide the reward in proportion to their difficulty. The reward counted
includes the block reward, the transaction fees and tips and the fees of private transfers. It is
credited once the block is 60 deep and still in the best chain, after a pool fee (1% by default),
and paid automatically above a minimum.

The miner is a build of XMRig 6.26 for `rx/tess`, CPU only, with no developer fee, published with
static binaries for Linux, macOS and Windows. An Apple M1 with 4 threads computes about 600 H/s
with it in fast mode; machines with less than 2.5 GB of free memory mine in light mode.
