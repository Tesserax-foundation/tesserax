---
title: "1. Introduction"
description: "Why Tesserax exists: the quantum threat to blockchains and the rise of autonomous agents."
---

# 1. Introduction

## 1.1 The quantum threat to public ledgers

A blockchain account is a public key, and a transaction is a signature that only the holder of the
matching private key can produce. Almost every chain in production — Bitcoin, Ethereum and their
descendants, and most of the rest — derives that security from the discrete logarithm problem on
an elliptic curve: secp256k1 for ECDSA and Schnorr, Curve25519 for Ed25519, Ristretto for sr25519.

Shor's algorithm solves the discrete logarithm problem in polynomial time on a fault-tolerant
quantum computer. Once such a machine exists, anyone who knows a public key can compute its private
key and spend what it holds. Three properties of blockchains make this worse than for most systems:

1. **Public keys are public forever.** Every signature published on chain reveals the signer's
   public key, and the ledger never forgets. Addresses that hash the public key only hide it until
   their first spend; reused addresses, early pay-to-public-key outputs and every account-based
   chain expose it outright.
2. **Harvest now, forge later.** An attacker does not need the quantum computer today: the keys
   are already recorded. The day the computer is ready, every exposed balance becomes spendable
   by it at once.
3. **Migration is a social problem.** Moving a running chain to new signatures requires every
   holder to move their funds in time, and the chain to decide what happens to the coins whose
   owners did not — lost keys, dormant wallets, estates. Every such decision is contentious, and it
   must be taken under the pressure of an attack that is already possible.

NIST standardized post-quantum signatures and key encapsulation in 2024: ML-DSA (FIPS 204,
derived from CRYSTALS-Dilithium) and ML-KEM (FIPS 203, from CRYSTALS-Kyber), whose security rests
on lattice problems (Module-LWE and Module-SIS) for which no efficient quantum algorithm is known.
Their cost is size: an ML-DSA-87 public key is 2,592 bytes and a signature 4,627 bytes, against 32
and 64 bytes for Ed25519. A chain that adopts them pays in block space and must be designed
around it.

Tesserax takes the position that the only migration that does not fail is the one that never has
to happen: a chain that is post-quantum from its genesis block, in every layer, with no classical
key that a future attacker could target.

## 1.2 Agents as economic actors

Software agents built on large language models now plan, call tools and act on behalf of people
and companies. The moment they need to pay — for an API, for compute, for another agent's work —
they hit the limits of the financial system: bank accounts and cards are tied to legal persons,
require human checks and are priced for transactions of dollars, not fractions of a cent.

Blockchains remove the identity barrier, but a plain key is the wrong tool for an agent. A key
either holds funds, and then a compromised or misbehaving agent can drain them, or it holds none,
and then the agent cannot act. What an owner needs is to give an agent a **budget**, not a
balance: a spending limit per period, a cap per payment, a list of allowed counterparties and an
expiry, enforced by the system itself and revocable at once. Agents also need ways to find each
other, to judge whom to trust, to pay for services one request at a time without a transaction
per request, and to contract work with guarantees on both sides.

Tesserax builds these into the runtime of the chain rather than into contracts deployed on top:
they are protocol rules, uniform for every agent, cheaper to execute and covered by the same
review and governance as the rest of the chain.

## 1.3 Design goals

**Post-quantum everywhere.** Every cryptographic assumption on which funds depend must hold
against a quantum adversary: signatures, peer-to-peer key exchange, the public RPC, the privacy
layer and the cryptography exposed to contracts. Classical primitives that cannot be made safe
are removed rather than deprecated.

**Fair, open issuance.** New coins enter circulation through proof of work that any computer can
perform, so that no one needs permission or prior holdings to obtain them. 85% of the supply is
mined; the remaining 15% is locked by vesting rules in the genesis block.

**Native agent rails.** Delegated spending, reputation, micropayments and task escrow are runtime
modules with fixed rules and benchmarked costs, reachable from any language model through MCP.

**Privacy as an option, not an afterthought.** Value can move into a shielded pool where sender,
recipient and amount are hidden, with proofs that rely on hashes only.

**No privileged keys.** There is no sudo account. The runtime changes only through referenda
voted with TESS, with a technical committee limited to fast-tracking what token holders still
have to approve.

**Interoperability without bridges.** Value moves between Tesserax and other chains through
atomic swaps, where each side holds its own keys until the exchange completes.

## 1.4 Non-goals

- **Maximum throughput.** Post-quantum signatures are large. Tesserax accepts a base layer of
  about 30 transfers per second and moves high-frequency payments off chain into payment channels,
  where vouchers are not limited by block space (chapter 5).
- **EVM equivalence.** Contracts are written in Solidity but compiled to PolkaVM; EVM bytecode and
  Ethereum-format transactions are rejected, because they would bring ECDSA back (chapter 8).
- **Instant finality.** Proof of work gives probabilistic finality. Tesserax finalizes blocks 60
  deep (about 18 minutes) and rejects reorganizations below that depth (chapter 4).

## 1.5 Status

The reference implementation is open source and runs a public testnet: the node, runtime, wallets
(command line, web, browser extension and desktop), explorer, faucet, mining pool and miner, SDKs
in Rust, TypeScript and Python, and the MCP server. Runtime upgrades on the testnet already go
through referenda. The mainnet launches from a genesis described in chapter 12.
