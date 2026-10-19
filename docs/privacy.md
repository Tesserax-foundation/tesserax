# Private transactions

Tessera has an optional shielded pool. Transparent TESS can be moved into the pool, transferred
privately inside it and moved back out. Shielded transfers hide the sender, the recipient and the
amount.

Every cryptographic component is post-quantum:

| Component | Primitive |
|---|---|
| Validity proofs | STARK (Plonky3, FRI), hash based |
| Commitments, nullifiers, note tree | Poseidon2 over the Goldilocks field, 256-bit digests |
| Note encryption | ML-KEM-768 (FIPS 203) + ChaCha20-Poly1305 |
| Transparent side | ML-DSA-65 transactions |

There is no elliptic curve and no trusted setup.

**Status:** design and implementation in progress. The shielded pool must be audited by
cryptographers specialised in zero-knowledge proofs before it holds real value.

## Notation

- `F` is the Goldilocks field, `p = 2^64 - 2^32 + 1`. A digest is four field elements.
- `P` is the Poseidon2 permutation of width 12 over `F` with the Plonky3 Goldilocks constants
  (4 + 4 full rounds, 22 partial rounds, S-box `x^7`).
- `H(a, b, d)` hashes two digests `a`, `b` with the domain `d`: the first four elements of
  `P(a || b || [d, 0, 0, 0])`. The last four elements are the capacity, so `H` is a sponge with a
  256-bit capacity absorbing one block.

Domains: `NK = 1`, `TAG = 2`, `CM_OWNER = 3`, `CM_VALUE = 4`, `NF = 5`, `NODE = 6`,
`RHO = 7 + j` for output `j`.

## Keys and addresses

A shielded wallet is derived from a 32 byte seed `s`:

- nullifier key `nk = H(s, 0, NK)`, where `s` is read as four field elements;
- owner tag `tag = H(nk, 0, TAG)`;
- an ML-KEM-768 key pair generated from SHA3-512 of the seed.

A shielded address is `tag || ek` (32 + 1184 bytes), `ek` being the ML-KEM encapsulation key.
Knowing `nk` is required to spend; knowing the ML-KEM decapsulation key is enough to see
incoming notes (incoming viewing key).

## Notes

A note is `(v, tag, rho, r)`:

- `v` is the value in planck, `v < 2^60` (about 1.15 million TESS per note);
- `tag` is the owner tag of the recipient;
- `rho` is a digest that makes the note unique;
- `r` is three random field elements that hide the note.

Commitment: `cm = H(H(tag, rho, CM_OWNER), [v, r0, r1, r2], CM_VALUE)`.

Nullifier: `nf = H(nk, cm, NF)`. Spending a note publishes its nullifier, which is recorded
on-chain: a note can be spent once, and the nullifier reveals nothing about the note.

Commitments are appended to an incremental Merkle tree of depth 32 (`node = H(left, right, NODE)`,
empty leaves are zero). The chain keeps the recent roots; a proof refers to one of them, its
anchor.

## Transactions

Every shielded transaction spends two notes and creates two notes. Unused inputs are dummy notes
of value 0, unused outputs are notes of value 0 to a random tag. Public data:

- `anchor`, `nf_1`, `nf_2`, `cm'_1`, `cm'_2`;
- `value_in`: amount moved from a transparent account into the pool;
- `value_out`: amount moved from the pool to a transparent account, and its recipient;
- `fee`;
- the two encrypted notes.

The proof shows, for the private witness (`nk_i`, input notes, Merkle paths, output notes):

1. for each input `i`: `tag_i = H(nk_i, 0, TAG)`, `cm_i` is the commitment of the input note,
   `nf_i = H(nk_i, cm_i, NF)`, and if `v_i != 0` the Merkle path of `cm_i` leads to `anchor`;
2. for each output `j`: `rho'_j = H(nf_1, nf_2, RHO + j)` and `cm'_j` is the commitment of the
   output note;
3. every note value is below `2^60`;
4. `v_1 + v_2 + value_in = v'_1 + v'_2 + value_out + fee`.

The output `rho` depends on the input nullifiers, which are unique: two outputs can never share a
commitment (no "faerie gold"). Values are below `2^60` and public amounts are checked below
`2^60` by the chain, so the balance equation cannot wrap around the field modulus.

The proof is bound to every public value through the Fiat-Shamir transcript, including the
transparent recipient and the hash of the encrypted notes: a relayer cannot redirect funds or
replace the ciphertexts.

The owner of the input notes authorises the spend by proving knowledge of `nk`: no additional
signature is needed. Shielded transfers are submitted without a transparent signer, so that they
are not linked to any account; their fee is paid from the pool.

## Note encryption

For each output, the sender encapsulates to the recipient's ML-KEM key, derives a key with
SHA3-256 and encrypts `v || rho || r` with ChaCha20-Poly1305. The ciphertext (1144 bytes) is
published with the transaction. Wallets try to decrypt every new note with their decapsulation
key and check the decrypted note against its commitment.

## Chain rules

- Nullifiers are stored; a transaction spending a known nullifier, or the same nullifier twice,
  is rejected.
- The anchor must be one of the last 100 roots.
- Turnstile: the chain tracks the total value in the pool. `value_in` adds to it, `value_out` and
  `fee` are taken from it, and it can never become negative. A flaw in the proof system could not
  create TESS outside the pool.
- Public amounts and fees must be below `2^60`.
