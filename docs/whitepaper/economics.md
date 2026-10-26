---
title: "5. Economics"
description: "Fees, block space and throughput, the emission schedule, and the team and foundation allocations."
---

# 5. Economics

## 5.1 Resources: weight and length

Every block has two budgets:

| Resource | Limit per block | Normal transactions | Operational |
|---|---|---|---|
| Weight (execution time) | 2 s of reference computation | 75%, 1.5 s | the remaining 25% |
| Length (bytes) | 5 MiB (5,242,880 bytes) | 75%, 3.75 MiB | the full 5 MiB |

Weight is measured in picoseconds of execution on reference hardware; every call has a weight
benchmarked for its worst case. Operational transactions, such as applying a runtime upgrade
authorized by referendum, can use the share of the block that normal transactions cannot, so that
a full block never prevents governance from acting.

## 5.2 Fees

The fee of a transaction is computed from its weight and its length, as in the Polkadot SDK, with
deliberately simple parameters:

$$
\text{fee} = \underbrace{w_{\text{base}}}_{\text{base}} + \underbrace{\ell}_{\text{length}} + m \cdot \underbrace{w_{\text{call}}}_{\text{weight}} + \text{tip}
$$

- Weight converts to planck one to one: one picosecond of execution costs one planck
  ($10^{-12}$ TESS).
- Length converts one to one as well: each byte costs one planck.
- The multiplier $m$ is fixed at 1: there is no fee market that raises fees with congestion. When
  blocks are full, transactions with a tip are included first; the priority of operational
  transactions is further multiplied by 5.
- The base weight of every transaction, about 0.98 ms, includes the verification of its ML-DSA-87
  signature. The base fee is therefore about **0.00098 TESS**.

A simple transfer costs the base fee, plus its execution (about 0.3 ms with its storage accesses),
plus about 7,300 planck for its 7.3 kB of length: **about 0.0013 TESS**. Signature verification
dominates; the length of the transaction, although large, is cheap.

All fees and tips go to the miner of the block. So do the fees of private transfers, the dust of
accounts removed below the existential deposit, the rounding of contract gas and the deposits
slashed by governance. Nothing is burned.

### Storage deposits

Storing data on chain requires a deposit, held from the account that stores it and returned when
the data is removed. Across the runtime, deposits follow one rule:

$$
\text{deposit}(\text{items}, \text{bytes}) = 0.02\ \text{TESS} \times \text{items} + 0.0001\ \text{TESS} \times \text{bytes}
$$

so that the cost of state is borne by those who create it, not by every future node.

## 5.3 Throughput

Post-quantum signatures make transactions large. An ML-DSA-87 transfer carries a 2,592-byte
public key and a 4,627-byte signature: about 7.3 kB in total, against about 200 bytes for a
classical transfer.

| | Per transfer | Per block (18 s) | Per second |
|---|---|---|---|
| Length | about 7.3 kB | 3.75 MiB: about **540** transfers | about **30** |
| Execution | about 1 ms | 1.5 s: about 1,500 transfers | about 83 |
| Private transfer | about 185 kB | about **20** | about 1 |

Length is the binding limit: about 30 transfers per second on chain. Tesserax does not try to
remove that limit at the base layer; it moves frequent payments off chain. A payment channel
(chapter 6) costs one transaction to open and one to claim, and in between carries any number of
payments as vouchers signed off chain, verified in about a millisecond by the receiving service.
An agent paying for an API call by call, thousands of times a day, uses two transactions.

## 5.4 Supply

The supply of TESS is capped at **14,000,000**:

| Share | Amount | How it is released |
|---|---|---|
| Mining, 85% | 11,900,000 TESS | Block rewards, halving every 7,000,000 blocks |
| Team, 10% | 1,400,000 TESS | Nothing for 6 months, then 12.5% at once and linearly until month 48 |
| Foundation, 5% | 700,000 TESS | 140,000 at genesis, 560,000 linearly over 36 months |

There is no pre-sale, no other genesis balance and no mechanism that can mint beyond these
amounts.

## 5.5 Emission

Each block mints a reward to the account in its proof-of-work pre-runtime digest, before any
transaction executes. For block $n \ge 1$:

$$
\text{reward}(n) = \left\lfloor \frac{0.85\ \text{TESS}}{2^{\lfloor (n-1) / 7{,}000{,}000 \rfloor}} \right\rfloor
$$

computed as a right shift on planck amounts. The genesis block has no reward. The runtime also
keeps the total minted and caps each reward at what remains of the 11,900,000 TESS, so the mining
supply can never be exceeded, whatever the rounding.

| Era | Blocks | Reward | Minted in era | Cumulative | Ends after |
|---|---|---|---|---|---|
| 1 | 1 – 7,000,000 | 0.85 | 5,950,000 | 5,950,000 | ~4 years |
| 2 | 7,000,001 – 14,000,000 | 0.425 | 2,975,000 | 8,925,000 | ~8 years |
| 3 | 14,000,001 – 21,000,000 | 0.2125 | 1,487,500 | 10,412,500 | ~12 years |
| 4 | 21,000,001 – 28,000,000 | 0.10625 | 743,750 | 11,156,250 | ~16 years |
| 5 | 28,000,001 – 35,000,000 | 0.053125 | 371,875 | 11,528,125 | ~20 years |
| … | | | | → 11,900,000 | |

At 18 seconds per block, a year holds about 1,753,000 blocks: the first era mints about 1.49
million TESS a year. Half of the mining supply is created in the first four years, three
quarters in eight. The schedule converges to 11,899,999.999867 TESS, a few ten-thousandths of a
TESS below the cap, after the last era whose reward is still at least one planck.

After the emission, miners are paid by fees. The fixed multiplier keeps fees predictable for
users; a referendum can change the fee parameters if the security budget requires it.

## 5.6 Allocations and vesting

The team and foundation allocations exist from genesis and are locked by vesting schedules that
the runtime enforces. A month is counted as 146,100 blocks (30.44 days of 18-second blocks).
Locked TESS cannot be transferred, cannot pay fees and **cannot vote** (chapter 10): the voting
power of the allocations grows only as they vest.

**Team — 1,400,000 TESS**, 700,000 to each of two members.

- Nothing is released during the first 6 months (876,600 blocks).
- At the cliff, the share that would have vested linearly is released at once: 12.5%.
- The rest is released block by block until month 48 (7,012,800 blocks).

$$
\text{unlocked}_{\text{team}}(n) = \begin{cases}
0 & n < 876{,}600 \\[2pt]
A \cdot \dfrac{\min(n, 7{,}012{,}800)}{7{,}012{,}800} & n \ge 876{,}600
\end{cases}
$$

**Foundation — 700,000 TESS**, held by a 2-of-3 multi-signature account whose signatories are
the two team members and a backup key kept offline.

- 140,000 TESS are liquid at genesis, to fund development, audits, listings and infrastructure.
- 560,000 TESS are released linearly over 36 months (5,259,600 blocks), without cliff.

Any two of the three keys approve a spending of the foundation; one lost key blocks nothing and one
stolen key is not enough. The same three keys form the technical committee (chapter 10), whose
power is limited to fast-tracking proposals that token holders still have to approve.

| Months after genesis | 0 | 6 | 12 | 24 | 36 | 48 |
|---|---|---|---|---|---|---|
| Team unlocked | 0% | 12.5% | 25% | 50% | 75% | 100% |
| Foundation unlocked | 20% | 33% | 47% | 73% | 100% | 100% |
| Mined (approx.) | 0 | 0.75 M | 1.49 M | 2.98 M | 4.47 M | 5.96 M |
