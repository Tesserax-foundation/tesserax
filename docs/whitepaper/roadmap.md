---
title: "12. Roadmap"
description: "From the public testnet to the mainnet genesis."
---

# 12. Roadmap

## 12.1 Done

| Milestone | Content |
|---|---|
| **Public testnet** | Post-quantum node and runtime, RandomTESS mining, wallets, explorer, faucet, mining pool and miner |
| **Agent economy** | Delegations, registry and reputation, payment channels with HTTP 402, task market, MCP server |
| **Privacy** | Shielded pool with STARK proofs, in every wallet and SDK |
| **Contracts** | Solidity on PolkaVM with the ML-DSA precompile |
| **Governance** | Sudo removed (runtime 103); referenda, conviction voting and the technical committee |
| **Atomic swaps** | TESS against BTC, ETH, USDC and USDT (runtime 104), enacted by the first referendum |

## 12.2 Before mainnet

1. **External audits** of the runtime, the post-quantum networking, the shielded pool and the
   wallets.
2. **Benchmarks on reference hardware**, and runtime weights updated accordingly.
3. **Wallet distribution**: the browser extension in the Chrome and Firefox stores, the desktop
   application signed for macOS and Windows.
4. **SDK releases** on crates.io, npm and PyPI.
5. **Swap contracts** deployed on Ethereum, Base and Arbitrum, and their test networks.
6. **Initial difficulty** set from the hash rate measured on the testnet.

## 12.3 Launch

The mainnet starts from the `mainnet` genesis of the runtime:

- 14,000,000 TESS maximum supply: the team and foundation allocations under vesting, and no other
  balance;
- no sudo: governance from block one, with the three foundation signatories as technical committee;
- an initial difficulty matching the expected hash rate.

The launch procedure is public: the runtime is built reproducibly and its hash published; the chain
specification is generated from it and frozen; bootnodes are added; the genesis hash and the launch
time are announced in advance, so that every miner starts at the same moment.

## 12.4 After launch

- Post-quantum certificates for the public RPC as soon as certificate authorities issue them.
- Larger anonymity sets and multi-input private transfers.
- Agent features driven by usage: richer policies, recurring payments, standard agent cards.
- Listings on exchanges and market-data aggregators, fed by the explorer's market endpoints.

Every change to the protocol after genesis goes through governance.
