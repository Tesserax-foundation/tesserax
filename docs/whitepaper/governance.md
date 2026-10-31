---
title: "10. Governance"
description: "Referenda with conviction voting, the technical committee and its whitelist, and runtime upgrades without privileged keys."
---

# 10. Governance

Tesserax has **no sudo key**: no account can change the chain by itself. The runtime and its
parameters change only by referenda voted with TESS, in a system derived from the OpenGov design of
Polkadot.

## 10.1 Referenda

Anyone may submit a proposal — an encoded call, stored as a preimage — with a submission deposit of
10 TESS. A proposal enters its **decision period** once a decision deposit is placed. It passes if,
before the period ends, both of its curves are met for a whole **confirmation period**:

- **approval**: the share of aye votes among aye and nay;
- **support**: the share of aye votes among all TESS that may vote.

| | Root | Urgent | Cancellation |
|---|---|---|---|
| Use | Any call: upgrades, parameters | Calls whitelisted by the committee | Cancel or kill a referendum |
| Decision deposit | 1,000 TESS | 500 TESS | 100 TESS |
| Preparation | 2 hours | 30 minutes | 1 hour |
| Decision period | 14 days | 2 days | 1 day |
| Confirmation | 1 day | 10 minutes | 3 hours |
| Minimum enactment | 1 day | 10 minutes | 10 minutes |
| Approval | 100% → 50% | 100% → 50% | 100% → 50% |
| Support | 20% → 1% | 5% → 0.5% | 10% → 0.5% |
| Referenda decided at once | 5 | 10 | 10 |

Both curves decrease linearly over the decision period: a proposal with overwhelming support passes
early, a contested one needs more time to gather turnout. Deposits are returned after the decision,
except for referenda killed as abusive, whose deposits go to the miner.

## 10.2 Conviction voting

A voter may lock its TESS beyond the referendum to multiply its vote:

| Conviction | Vote weight | Lock after the referendum |
|---|---|---|
| 0 | 0.1× | none |
| 1 | 1× | 1 week |
| 2 | 2× | 2 weeks |
| 3 | 3× | 4 weeks |
| 4 | 4× | 8 weeks |
| 5 | 5× | 16 weeks |
| 6 | 6× | 32 weeks |

Those who commit longer to the outcome weigh more. Votes can be delegated per track.

**TESS still vesting cannot vote.** The voting balance of an account excludes its locked vesting
schedule, and the support curve is measured against the votable issuance — the total issuance minus
what the genesis allocations still vest. The team and the foundation thus gain voting power only as
their allocations unlock, and early miners are not outvoted by locked allocations. Delegated agent
keys can never vote with their owner's TESS.

## 10.3 The technical committee and the whitelist

Some changes cannot wait 14 days — a fix for a vulnerability, for instance. The **technical
committee** — three members, fixed at genesis as the signatories of the foundation, with no call
that can change them — can **whitelist** a call with two of three votes. A whitelisted call can then
go through the **urgent track**: 2 days of decision, 10 minutes of confirmation.

The committee cannot dispatch anything itself. A whitelisted call still needs a referendum, with its
approval and support, to pass. The committee's power is to shorten the path of what token holders
approve, not to replace them.

## 10.4 Runtime upgrades

Upgrading the runtime takes two steps, so that a large code blob never has to fit in a referendum
or a scheduled call:

1. A referendum enacts `System.authorize_upgrade(hash)`, where hash is the BLAKE2-256 hash of the
   new WebAssembly runtime. The runtime is built reproducibly, so anyone can rebuild it from the
   tagged sources and check the hash before voting.
2. Once authorized, anyone submits `System.apply_authorized_upgrade(code)`: an unsigned transaction,
   without fee, refused unless the code hashes to the authorized value. The chain runs the new
   runtime from the next block.

Nodes need no update for a runtime upgrade: they execute the runtime stored on chain.

### History of the testnet

| Version | Date | Change |
|---|---|---|
| 100 | 2026-10-23 | Genesis |
| 101 | 2026-10-25 | An address in contracts belongs to its first owner |
| 102 | 2026-10-25 | Reproducible build of the same runtime |
| 103 | 2026-10-25 | Governance replaces sudo |
| 104 | 2026-10-26 | Atomic swaps — the first upgrade by referendum, on the urgent track |
| 105 | 2026-10-27 | Periods counted in exact hours and days |
| 106 | 2026-10-31 | Bids: sellers sell TESS at once into the bids of buyers |

## 10.5 Limits

A root referendum can change anything, including the rules of governance itself; the protections
are the length of its periods, the deposits, the support required and the visibility of every
proposal. Vesting TESS cannot vote but can fund deposits. Low turnout is the main risk of any
on-chain governance; the decreasing support curves set a floor of 1% of votable issuance on the
root track.
