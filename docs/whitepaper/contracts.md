---
title: "8. Smart contracts"
description: "Solidity contracts on PolkaVM, without EVM, with the elliptic-curve precompiles removed and an ML-DSA verification precompile."
---

# 8. Smart contracts

Tesserax runs smart contracts with `pallet-revive`, executing PolkaVM programs — a virtual machine
based on the RISC-V instruction set, compiled ahead of time to native code on the node. Contracts
are written in **Solidity** and compiled with `resolc`, which translates Solidity's intermediate
representation to RISC-V. Developers keep the language, the ABI and most of the tooling of
Ethereum; the chain keeps none of its elliptic-curve cryptography.

## 8.1 No EVM

| Path | Status |
|---|---|
| Upload PolkaVM code, instantiate, call (`revive.instantiate_with_code`, `revive.call`), signed with ML-DSA | Allowed |
| EVM bytecode | Rejected at upload |
| Ethereum-format transactions (`eth_transact`, `eth_call`, …) | Rejected by the call filter |

Ethereum transactions are signed with ECDSA over secp256k1; accepting them would reintroduce
exactly the keys Tesserax exists to avoid. Contracts are therefore deployed and called with
ordinary Tesserax transactions, and `msg.sender` is the contract address of an ML-DSA account.

## 8.2 Precompiles

Precompiles that rely on elliptic curves are removed, because a contract that trusts them would be
insecure against a quantum attacker:

| Precompile | Status |
|---|---|
| `sha256`, `ripemd160`, `identity`, `modexp`, `blake2f` | Available |
| `ecrecover`, BN128 add / mul / pairing, P-256 verification, KZG point evaluation | Removed |
| `sr25519_verify`, `ecdsa_recover`, `ecdsa_to_eth_address` (contract API) | Always fail |
| **ML-DSA-87 verification** at `0x00000000000000000000000000000000d5a00000` | Added |

```solidity
interface IMlDsa87 {
    function verify(
        bytes calldata publicKey,
        bytes calldata context,
        bytes calldata message,
        bytes calldata signature
    ) external view returns (bool);
}
```

The precompile lets contracts verify post-quantum signatures — for multisig wallets, signed orders,
meta-transactions or oracle reports — at a cost of about twice the measured verification time plus
a per-byte charge for hashing the message. It returns `false` for malformed input, and always for
the context `tesserax`, so that a contract can never be presented a transaction signature.

## 8.3 Addresses

Contracts use 20-byte addresses. An account's contract address is the last 20 bytes of the
Keccak-256 hash of its 32-byte identifier; accounts are mapped to their address automatically when
they are created.

A 160-bit address raises a question that 256-bit identifiers do not: two keys whose addresses
collide would control the same contract identity. Finding some collision costs about $2^{80}$
operations classically; finding an address that collides with a given account costs about
$2^{160}$ classically and about $2^{80}$ with Grover's algorithm. Tesserax makes such a collision
worthless: **an address belongs to its first owner.** Mapping refuses an address already mapped to
another account or holding a contract, a contract cannot be created at an account's address, and an
account acts in contracts only through the address mapped to it. A second key reaching the same 160
bits gains nothing.

## 8.4 Parameters

| Parameter | Value |
|---|---|
| Storage deposit per item | 0.02 TESS |
| Storage deposit per byte | 0.0001 TESS |
| Code lockup deposit | 30% of the code's storage deposit |
| Runtime memory | 128 MiB |
| `block.chainid` | 2261 |
| Decimals seen by contracts | 18 (1 TESS = $10^{18}$ wei-like units) |
| Gas | Mapped to weight; gas rounding goes to the miner |

Storage deposits are held from the caller for the state a call creates and refunded when the state
is cleared, so contracts pay for the state they keep. The SDKs dry-run every call and add a 10%
margin to both weight and deposit limits.

## 8.5 Agents and contracts

A delegated agent may call existing contracts when its policy allows it (chapter 6), but may not
deploy new ones. Value sent to a contract and deposits taken by it count against the agent's budget,
because the delegation measures the change of the owner's balance rather than the arguments of the
call.
