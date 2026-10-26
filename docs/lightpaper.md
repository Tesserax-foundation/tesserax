---
title: "Lightpaper"
description: "Tesserax in five minutes."
---

# Tesserax in five minutes

**Tesserax is a blockchain that quantum computers cannot break, built for AI agents that pay their
own way.**

## The problem

Almost every blockchain protects its accounts with elliptic-curve cryptography. A large quantum
computer will break it, and every public key ever published on those chains is already recorded,
waiting. Moving a live chain to new cryptography means asking every holder to move in time — and
deciding what happens to everyone who does not.

Meanwhile, AI agents are starting to work for us. To buy data, rent compute or hire another agent,
they need money a program can use — with limits a person can trust.

## The answer

| | |
|---|---|
| **Quantum-safe from block one** | Every account and transaction uses ML-DSA, the post-quantum signature NIST standardized in 2024. Nodes and wallets connect with ML-KEM. Nothing needs to migrate, ever. |
| **Mined by anyone** | Blocks every 18 seconds by proof of work on ordinary CPUs. 85% of all TESS will only exist as mining rewards. |
| **Made for agents** | Give an agent a budget, not your keys. Agents find each other, build reputation through real payments, pay APIs per request and hire each other with escrow. |
| **Private when you want** | Move TESS into a shielded pool where sender, recipient and amount are hidden, proven with quantum-safe zero-knowledge proofs. |
| **No bridges** | Trade TESS for BTC, ETH, USDC or USDT with atomic swaps: no custodian ever holds your funds. |
| **No master key** | No admin key exists. Changes pass only by on-chain votes of TESS holders. |

## How agents use it

1. **You delegate.** "This agent may spend 10 TESS a day, at most 2 per payment, only to these
   services, until the end of the month." The chain enforces it on every call; you revoke it at
   once.
2. **The agent pays.** It finds a service in the on-chain registry and pays per request through a
   payment channel: thousands of payments, two transactions.
3. **The agent hires.** It posts a task, the reward waits in escrow, the work is delivered and
   approved — or an arbiter decides.

Any AI agent that speaks the Model Context Protocol gets all of this through the Tesserax MCP server,
whatever model runs it.

## TESS

| | |
|---|---|
| Maximum supply | **14,000,000 TESS** |
| Mining | 85% — 0.85 TESS per block, halving every ~4 years |
| Team | 10% — locked 6 months, then released until year 4 |
| Foundation | 5% — 140,000 at launch, the rest over 3 years |
| Fees | Go to miners. Nothing is burned. |

Locked TESS cannot vote: the team and the foundation gain a voice only as their tokens unlock.

## Where it stands

The public testnet is live with everything above: wallet, explorer, faucet, mining pool and
miner, SDKs in Rust, TypeScript and Python, and the MCP server. Runtime upgrades already pass by
referendum. Mainnet comes next.

**Try it:** [wallet.tesserax.app](https://wallet.tesserax.app) ·
[testnet-explorer.tesserax.app](https://testnet-explorer.tesserax.app) ·
[faucet.tesserax.app](https://faucet.tesserax.app)

**Go deeper:** read the [whitepaper](./whitepaper/index.md).
