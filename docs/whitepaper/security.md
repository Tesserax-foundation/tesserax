---
title: "11. Security analysis"
description: "The threat model of Tesserax, the attacks it considers, its defenses and its known limits."
---

# 11. Security analysis

## 11.1 Threat model

Tesserax assumes an adversary that:

- may have a **cryptographically relevant quantum computer**, now or in the future, and has recorded
  everything published on chain and on the network;
- may control **a minority of the hash power**, and temporarily a majority;
- controls some **network peers** and can delay or reorder messages, but cannot partition honest
  nodes for long;
- may **compromise agent keys** or manipulate agents through their inputs (prompt injection);
- may run **malicious services, contracts and counterparties**.

It assumes that the lattice problems behind ML-DSA and ML-KEM remain hard for quantum computers,
that the hash functions used behave as random oracles at their security levels, and that at least
one implementation of each primitive is correct.

## 11.2 Quantum attacks

| Target | Classical chain | Tesserax |
|---|---|---|
| Spending from a revealed public key | Shor recovers the private key | ML-DSA-87: no known quantum attack |
| Forging transactions | Same | Same |
| Decrypting recorded network traffic | Classical key exchange broken retroactively | ML-KEM-768 on every peer link and RPC session |
| Impersonating peers | Ed25519 identities | ML-DSA-87 identities |
| Breaking the privacy pool | Pairing-based SNARKs: forgery and deanonymization | Hash-based STARK; ML-KEM note encryption |
| Contract-level signature checks | `ecrecover` | Removed; ML-DSA precompile |
| Proof of work | Grover gives at most a quadratic speed-up on hashing, impractical against memory-hard RandomX | Same |

One classical dependency remains, outside the protocol: the other side of atomic swaps, which is
as safe as Bitcoin or Ethereum.

## 11.3 Consensus attacks

**Majority attack.** An attacker with more than half of the hash power can produce the best chain:
censor transactions and reverse its own recent payments. Finality at 60 blocks bounds the reversal
to about 18 minutes of history; services should wait for finality before releasing goods of value.
RandomTESS keeps mining on CPUs, which widens the base of miners, and its distinct configuration
prevents renting hash power from other RandomX networks without changing software.

**Selfish mining and block withholding.** Withholding blocks yields a higher share of rewards above
a threshold of hash power, as in every proof-of-work chain. The 60-block finality limits how long
a private chain can be held back.

**Timestamp manipulation.** Timestamps must exceed the parent's by at least one second and lead the
importing node's clock by at most 30 seconds; the LWMA caps every solve time at 108 seconds and any
increase at 10 times the window average. A miner cannot drive the difficulty far from the actual
hash power.

**Long-range reorganizations.** Nodes never leave a finalized block. A node isolated for more than
60 blocks may finalize a minority chain and must be resynchronized by its operator — the price of
bounded reorganizations.

**Eclipse attacks.** A node whose peers are all controlled by an attacker sees only the attacker's
chain. Mutual ML-DSA authentication prevents impersonating known peers; operators should connect to
several bootnodes and independent peers.

## 11.4 Transaction-level attacks

- **Replay** across chains and runtimes is prevented by the genesis hash and versions in every
  signed payload, and within a chain by nonces and mortality. Vouchers sign the genesis hash; swap
  claims are one-shot; shielded spends are bound by nullifiers.
- **Fee-less spam.** The three unsigned calls are guarded: private transfers carry a fee checked
  before inclusion and are verified before entering the pool; swap claims require the secret of an
  existing trade; upgrade applications require a code matching an authorization.

## 11.5 Agent attacks

| Attack | Defense |
|---|---|
| Stolen agent key | Loss bounded by the budget per period and the cap per call, to allowed targets only, until revocation |
| Prompt injection making an agent overspend | Limits enforced by the chain, not by the agent or the MCP server |
| Agent escalating to governance or vesting | Closed call classification: unclassified calls are refused |
| Reputation farming | Ratings require payments of at least 0.01 TESS each, one per payment; payers counted per account |
| Service overcharging through vouchers | Payer refuses prices above its maximum and claims of more than was paid; vouchers are cumulative |
| Payer closing a channel before paying | Close period of 6 hours to claim; services refuse channels closing within 100 blocks |
| Worker not delivering | Bond lost to the client after the deadline |
| Client refusing to pay for delivered work | Silence is approval; disputes go to the arbiter named at creation |

## 11.6 Privacy attacks

- **Double spending** of a note is prevented by its nullifier, checked against storage and between
  pending transactions.
- **Forged proofs** would require breaking the STARK; the turnstile limits the damage to value
  already in the pool.
- **Linking** through amounts, timing and the shape of transactions is reduced by the fixed
  two-in, two-out shape and a uniform fee, but shield and unshield amounts remain public, and the
  anonymity set is the tree at the anchor.

## 11.7 Implementation risks

- The ML-DSA implementations — RustCrypto `ml-dsa` in the runtime, AWS-LC in networking — are
  recent implementations of a recent standard; every SDK is checked against the same test vectors.
- Weights were benchmarked on a development machine and must be measured again on reference
  hardware before mainnet, so that block limits hold on slower machines.
- The shielded pool's circuit is new code; its turnstile bounds what a flaw could affect to the
  value inside the pool.

The foundation funds a bug bounty of 25,000 TESS, with rewards from 50 to 10,000 TESS by
severity, covering the chain, the wallets, the miner and the testnet services. Reports are made
privately through the security advisories of the repositories.
