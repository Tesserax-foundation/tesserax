---
title: "7. Privacy"
description: "The shielded pool of Tesserax — notes, commitments, nullifiers, ML-KEM note encryption and the zero-knowledge STARK that proves private transfers."
---

# 7. Privacy

Balances and transfers on Tesserax are public by default. The **shielded pool** lets holders move
TESS into a private set of notes, transfer them without revealing sender, recipient or amount, and
move them back out. Its design follows the note-and-nullifier model of Zerocash, with every
component chosen to be post-quantum:

| Component | Choice | Assumption |
|---|---|---|
| Proofs | STARK (Plonky3, FRI), zero-knowledge, transparent | Hash functions |
| Commitments, nullifiers, note tree | Poseidon2 over the Goldilocks field | Hash function |
| Note encryption | ML-KEM-768 + SHA3-256 + ChaCha20-Poly1305 | Module-LWE, symmetric |
| Public side | ML-DSA-87 transactions | Module-LWE/SIS |

There is no elliptic curve, no pairing and no trusted setup. Pools built on SNARKs over pairing
curves (Groth16, PLONK with KZG) lose both soundness and privacy to a quantum adversary; a STARK
relies only on the collision resistance of hashes.

## 7.1 Hash function

Inside proofs, everything is hashed with Poseidon2, a permutation designed for arithmetic circuits,
over the Goldilocks prime field:

$$
p = 2^{64} - 2^{32} + 1
$$

The permutation has width 12, 8 full rounds and 22 partial rounds with the S-box $x^7$. A digest
is 4 field elements (256 bits). The two-to-one hash with domain separation is a single-block
sponge with 256 bits of capacity:

$$
H(a, b, d) = \text{Poseidon2}\big(a_{[4]} \,\Vert\, b_{[4]} \,\Vert\, (d, 0, 0, 0)\big)_{[0..4]}
$$

with domains NK = 1, TAG = 2, CM_OWNER = 3, CM_VALUE = 4, NF = 5, NODE = 6 and RHO + $j$ = 7 + $j$.

## 7.2 Keys and addresses

A shielded spending key is a 32-byte seed. Wallets derive it from the account's ML-DSA seed, so the
24-word phrase of an account also restores its private notes:

$$
s = \text{BLAKE2-256}(\texttt{"tesserax-shielded-seed"} \,\Vert\, \text{seed}_{\text{ML-DSA}})
$$

From $s$:

$$
nk = H(s, 0, \text{NK}), \qquad \text{tag} = H(nk, 0, \text{TAG})
$$

$$
(dk, ek) = \text{ML-KEM-768.KeyGen}\big(\text{SHA3-512}(\texttt{"tesserax-shielded-kem"} \,\Vert\, s)\big)
$$

- $nk$, the **nullifier key**, is needed to spend.
- tag, the **owner tag**, identifies the owner inside commitments without revealing $nk$.
- $dk$, the ML-KEM decapsulation key, is the **incoming viewing key**: it decrypts the notes
  received, and cannot spend them.

A shielded address is $\text{tag} \,\Vert\, ek$ — 32 + 1,184 bytes — encoded in Base58 with a
4-byte BLAKE2 checksum and the prefix `zte`: about 1,670 characters, long because it contains an
ML-KEM public key.

## 7.3 Notes

A note is a value owned by a tag:

$$
\text{note} = (v, \text{tag}, \rho, r), \qquad v < 2^{60}\ \text{planck}, \quad r \in \mathbb{F}_p^3
$$

where $\rho$ makes the note unique and $r$ is randomness that hides the note in its commitment.
The pool stores only commitments:

$$
\text{cm} = H\big(H(\text{tag}, \rho, \text{CM\_OWNER}),\ (v, r_0, r_1, r_2),\ \text{CM\_VALUE}\big)
$$

Spending a note publishes its **nullifier**, which only the owner can compute and which reveals
nothing about which commitment it belongs to:

$$
\text{nf} = H(nk, \text{cm}, \text{NF})
$$

The output notes of a transaction take their $\rho$ from the nullifiers it spends, which makes them
unique even if a sender reuses everything else (the "faerie gold" attack, where two notes with
the same nullifier would let only one of them be spent):

$$
\rho'_j = H(\text{nf}_1, \text{nf}_2, \text{RHO} + j)
$$

### The note tree

Commitments are appended to a Merkle tree of depth 32 — up to $2^{32}$ notes — with
$\text{node} = H(\text{left}, \text{right}, \text{NODE})$. The chain stores only the frontier of the
tree (1 KiB) and the last 100 roots that changed it; a transaction proves membership against one
of those roots, its **anchor**, so that a proof stays valid while new notes arrive.

## 7.4 Transactions

Every shielded transaction has the same shape: **two inputs and two outputs**. Unused inputs are
dummy notes of value zero, and unused outputs are notes of value zero to fresh random addresses,
so the shape reveals nothing.

```text
ShieldedTransaction {
  anchor, nullifiers[2], commitments[2],
  value_in, value_out, fee, recipient?,
  encrypted_notes[2], proof
}
```

The public amounts connect the pool to the transparent world:

| Operation | Call | Signed | `value_in` | `value_out` | Public | Hidden |
|---|---|---|---|---|---|---|
| Shield | `shield` | yes | > 0 | 0 | sender, amount | recipient |
| Private transfer | `transact` | **no** | 0 | 0 | the fee | sender, recipient, amount |
| Unshield | `transact` | **no** | 0 | > 0 | recipient, amount | sender |

A private transfer has no signer at all: knowing $nk$, proven inside the STARK, authorizes it. It is
therefore linked to no account, not even through the account paying its fee — the fee is taken
from the pool, as a public input of the proof, and minted to the miner of the block. Every SDK pays
exactly the minimum fee, about 0.014 TESS, so the fee does not distinguish wallets either.

A **binding hash** ties the proof to everything the proof does not cover:

$$
\text{binding} = \text{BLAKE2-256}\big(\texttt{"tesserax-shielded-binding"},\ \text{source},\ \text{recipient},\ \text{notes}\big) \bmod p
$$

where source is the signer of a `shield` and none otherwise. The binding is a public input, so
replacing the recipient of an unshield, the encrypted notes, or replaying a `shield` from another
account invalidates the proof.

## 7.5 The statement

The STARK proves, for public inputs $(\text{anchor}, \text{nf}_1, \text{nf}_2, \text{cm}'_1, \text{cm}'_2, v_{\text{in}}, v_{\text{out}}, \text{fee}, \text{binding})$,
knowledge of private inputs — two notes, their Merkle paths, $nk$, two output notes — such that:

1. **Ownership.** For each input, $\text{tag} = H(nk, 0, \text{TAG})$ and the note's commitment
   uses this tag.
2. **Membership.** For each input of non-zero value, its commitment is a leaf of the tree whose
   root is the anchor.
3. **Nullifiers.** $\text{nf}_i = H(nk, \text{cm}_i, \text{NF})$.
4. **Outputs.** Each output's $\rho'_j = H(\text{nf}_1, \text{nf}_2, \text{RHO}+j)$ and its
   commitment equals $\text{cm}'_j$.
5. **Ranges.** Every value is decomposed into 60 bits: $0 \le v < 2^{60}$, so no sum wraps around
   the field.
6. **Balance.**

$$
v_1 + v_2 + v_{\text{in}} = v'_1 + v'_2 + v_{\text{out}} + \text{fee}
$$

### The AIR

The computation is laid out as an algebraic intermediate representation of 128 rows, each row one
Poseidon2 permutation: 36 rows per input (tag, two commitment rows, nullifier, 32 tree levels) and
3 per output. Periodic selector columns assign each row its role; constraints have degree 7, the
degree of the S-box, with no intermediate registers. There are 27 public values.

### The proof system

| Parameter | Value |
|---|---|
| Library | Plonky3 (`p3-uni-stark` 0.8) |
| Field | Goldilocks; challenges in its quadratic extension |
| Commitment | FRI over Merkle trees of Keccak-256, with salted leaves |
| Zero knowledge | Hiding FRI with 4 random codewords and salts |
| Blowup factor | 8 ($\log = 3$) |
| Queries | 38, with 16 bits of grinding |
| Fiat–Shamir | Keccak-256 challenger |

The conjectured soundness, under the ethSTARK conjecture, is

$$
\lambda \approx \log_2(\text{blowup}) \times \text{queries} + \text{grinding} = 3 \times 38 + 16 = 130 \text{ bits.}
$$

The extension field of about $2^{128}$ elements and 256-bit hashes keep the other terms at or above
128 bits; against a quantum attacker, Grover-style search on the hash reduces the margin but no
algorithm breaks the construction. Without hiding, a STARK reveals information about its witness;
the salted Merkle leaves and random codewords make the proof zero-knowledge, and two proofs of the
same transfer differ.

| Cost | Value |
|---|---|
| Proof size | about 182 kB (transaction about 185 kB) |
| Proving, Apple M1 | about 0.1 s, natively or in a browser through WebAssembly |
| Verification | about 12 ms in the runtime |
| Transfers per block | about 20, bounded by block length |

## 7.6 Validation

A shielded transaction is checked when it enters the transaction pool and again in the block that
includes it:

1. all amounts are below $2^{60}$;
2. without signer, $v_{\text{in}} = 0$ and the fee is at least the minimum; with a signer,
   $v_{\text{in}} > 0$;
3. a recipient is given if and only if $v_{\text{out}} > 0$, and can receive the amount;
4. the two nullifiers differ and neither is already spent;
5. the anchor is one of the recorded roots;
6. the tree has room for two more notes;
7. the pool balance covers $v_{\text{out}} + \text{fee}$;
8. the proof verifies.

Verification happens before inclusion, in the authorization of the call: once a transaction is
authorized, it cannot fail, so every included transaction pays its fee. Conflicting transactions
that spend the same note are resolved by the pool through their nullifiers.

The minimum fee prices the verification and the size of the transaction:

$$
\text{fee}_{\min} = w_{\text{base}} + w_{\text{verify}} + w_{\text{apply}} + 190{,}000\ \text{bytes} \approx 0.014\ \text{TESS}
$$

## 7.7 Accounting and the turnstile

Value entering the pool is burned from the sender's balance; value leaving it is minted to the
recipient; the fee is minted to the miner. The runtime tracks the **pool balance** — the total value
of the notes — and refuses any transaction that would make it negative:

$$
\text{pool}' = \text{pool} + v_{\text{in}} - v_{\text{out}} - \text{fee} \ \ge 0
$$

This turnstile bounds the damage of a flaw in the proof system: a forged proof could at worst move
value already inside the pool, never create TESS outside it. The total supply is the total issuance
of balances plus the pool balance, which the explorer publishes.

## 7.8 Wallets

A wallet follows the pool's events in order: it appends each new commitment to its local copy of
the tree, tries to decrypt each encrypted note with its viewing key — one ML-KEM decapsulation per
note — and marks its notes spent when their nullifiers appear. Each encrypted note is 1,168 bytes:
an ML-KEM-768 ciphertext of 1,088 bytes, then the 64-byte note plaintext and a 16-byte tag, sealed
with ChaCha20-Poly1305 under

$$
k = \text{SHA3-256}(\texttt{"tesserax-note-encryption"} \,\Vert\, \text{shared\_secret}).
$$

A note is accepted only if it decrypts and its commitment matches the one on chain. The same prover
runs natively in the command line wallet and the Rust SDK, compiled to WebAssembly in the web
wallet and the TypeScript SDK, and through WASI in the Python SDK.

## 7.9 Limits

- **Anonymity set.** A spent note can be any note of the tree at its anchor. The set grows with use
  of the pool; early on it is small, and timing and amounts of shield and unshield operations can
  link them.
- **Network privacy.** The pool hides the content of transactions, not the IP address that
  broadcasts them.
- **Two inputs.** Spending more than two notes takes several transactions.
- **Maturity.** The proof system is new code; the turnstile bounds what a flaw could affect to
  the value inside the pool.
