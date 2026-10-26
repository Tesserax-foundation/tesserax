---
title: "Tesserax Whitepaper"
sidebar_label: "Abstract"
description: "The technical whitepaper of Tesserax, a post-quantum proof-of-work blockchain built for AI agents."
---

# Tesserax: a post-quantum proof-of-work chain for autonomous agents

**Version 1.0 · October 2026 · Tesserax developers**

## Abstract

Two shifts are coming to public blockchains at once. The first is cryptographic: a large enough
quantum computer breaks the elliptic-curve signatures that protect nearly every account in
existence, and every public key ever revealed on chain stays exposed until then. The second is
economic: software agents built on language models are starting to buy, sell and pay for work on
their own, and they need money that a program can hold under limits a human can trust.

Tesserax is a layer-1 blockchain designed for both. Every account is an ML-DSA-87 key (FIPS 204)
and every transaction an ML-DSA signature; there is no elliptic-curve account, no classical
fallback and no migration to plan. Nodes and the public RPC exchange keys with ML-KEM-768
(FIPS 203). Private transfers are proven with hash-based zero-knowledge STARKs and their notes are
encrypted with ML-KEM, so the privacy layer carries no elliptic-curve or pairing assumption
either. Smart contracts run on PolkaVM, with the elliptic-curve precompiles removed and a native
precompile that verifies ML-DSA signatures.

Blocks are produced by proof of work every 18 seconds on average, with RandomTESS, a configuration
of RandomX v2 that keeps mining on general-purpose CPUs. The supply is capped at 14,000,000 TESS:
85% is only ever created as block rewards, halving every 7,000,000 blocks (about four years), and
the rest is held by the team and a foundation under vesting schedules enforced by the chain.

On top of this base, the runtime gives AI agents native economic primitives: delegations that let
an agent key spend an owner's funds within a budget, a per-call cap, allowed recipients and an
expiry; a registry of agents whose reputation can only be earned through payments; payment
channels whose vouchers pay HTTP services per request with status 402; and a task market with
escrow and arbitration. An MCP server exposes all of it to any agent that speaks the Model Context
Protocol. Atomic swaps trade TESS against BTC, ETH, USDC and USDT without bridges or custodians,
and the protocol evolves by on-chain referenda, without any privileged key.

This paper describes the design, its parameters and its security, as implemented in the
open-source reference implementation running on the public testnet.

## Contents

1. [Introduction](./introduction.md): the quantum threat, the agent economy, design goals.
2. [Architecture](./architecture.md): the node, the runtime and the clients.
3. [Cryptography](./cryptography.md): ML-DSA accounts, keys, post-quantum transport, hashing.
4. [Consensus](./consensus.md): RandomTESS, the seal, difficulty, fork choice and finality, mining.
5. [Economics](./economics.md): fees, throughput, emission, allocations.
6. [The agent economy](./agents.md): delegation, registry, channels and HTTP 402, tasks, MCP.
7. [Privacy](./privacy.md): the shielded pool and its STARK.
8. [Smart contracts](./contracts.md): PolkaVM, precompiles, addresses.
9. [Atomic swaps](./swaps.md): offers, hash time-locked contracts on Bitcoin and EVM chains.
10. [Governance](./governance.md): referenda, conviction voting, the technical committee, upgrades.
11. [Security analysis](./security.md): threat model, attacks and limits.
12. [Roadmap](./roadmap.md): from testnet to mainnet.
13. [References](./references.md).

## Notation

| Symbol | Meaning |
|---|---|
| TESS | The native token. 1 TESS = $10^{12}$ planck. |
| $T$ | Target block time, 18 s. |
| $D$ | Difficulty: the expected number of hashes to find a block. |
| $\mathcal{H}(x)$ | BLAKE2b-256 unless stated otherwise. |
| $x \,\Vert\, y$ | Concatenation of byte strings. |
| $H(a, b, d)$ | The Poseidon2 hash of the shielded pool (chapter 7). |

Parameters are given as configured in the runtime (spec version 105). Time expressed in hours or
days is converted to blocks at the 18-second target: 200 blocks an hour, 4,800 a day. Actual
durations vary with the time between blocks.

:::note
This document describes software. It is not an offer of any financial product, and the testnet
token has no value. Parameters may change by referendum; the code is the authority.
:::
