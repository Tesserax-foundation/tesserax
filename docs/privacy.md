# Private transactions

Tesserax has an optional shielded pool. Transparent TESS can be moved into the pool, transferred
privately inside it and moved back out. Shielded transfers hide the sender, the recipient and the
amount.

Every cryptographic component is post-quantum:

| Component | Primitive |
|---|---|
| Validity proofs | STARK (Plonky3, FRI), hash based |
| Commitments, nullifiers, note tree | Poseidon2 over the Goldilocks field, 256-bit digests |
| Note encryption | ML-KEM-768 (FIPS 203) + ChaCha20-Poly1305 |
| Transparent side | ML-DSA-87 transactions |

There is no elliptic curve and no trusted setup.

**Status:** active from the genesis, on the testnet and the mainnet. The pool balance can never
become negative, so that a flaw in the proof system could not create TESS outside the pool; the
pool must still be audited by cryptographers specialised in zero-knowledge proofs before it holds
much value.

## Using it

Every account has a shielded account, whose key derives from the seed of its ML-DSA key: its
recovery phrase restores its shielded notes too. Its shielded address starts with `zte` and is
long, about 1,670 characters, most of it the ML-KEM-768 encapsulation key notes are encrypted to.

```bash
tesserax-wallet shielded address alice              # the shielded address of alice
tesserax-wallet shielded shield alice 10            # 10 TESS of alice into her shielded balance
tesserax-wallet shielded send alice zte... 4        # 4 TESS to a shielded address, privately
tesserax-wallet shielded unshield alice te... 3     # 3 TESS of the shielded balance to an address
tesserax-wallet shielded balance alice
```

- Shielding is a transaction signed by the account: everyone sees the account and the amount
  entering the pool, not where they go.
- A private transfer is submitted without signer, linked to no account: its proof authorizes it,
  and its fee (the minimum fee, about 0.014 TESS) is paid from the shielded balance.
- Unshielding shows the amount and the transparent recipient, not the notes it comes from.
- A transaction spends at most two notes: an amount larger than the two largest notes is sent
  in steps, or after sending the whole balance to oneself.

The wallet reads the `Shielded.Transacted` events of every block since the genesis, keeps the note
tree of the chain and the notes of the account in its directory (`shielded/<account>.state`), and
reads the chain again from the genesis after a reorganization below its last block. Libraries:
`shielded-wallet` (keys, notes, synchronization and proving, native and WebAssembly) and the
`shielded` feature of the Rust SDK (`ShieldedAccount`).

The web wallet, its extension and its desktop application have a Private page: proofs are
generated in the browser by `shielded-wasm` (`scripts/build-shielded-wasm.sh`), in about 0.1
second. They read the transactions of the pool from the explorer database
(`shielded_transactions`, public data only) in one request and check the note tree against the
latest root on chain; without it, they read the blocks of the node.

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

## Proof system

The statement is an AIR of 128 rows (`primitives/shielded/src/proof`). Each row evaluates one
Poseidon2 permutation; periodic selector columns give each row its role (owner tag, commitment,
nullifier, Merkle level, output `rho`, output commitment, padding). Registers carry the
nullifier key, the Merkle accumulator, the input value and the running balance between rows.

Proofs use Plonky3 `uni-stark` over Goldilocks, with challenges in its quadratic extension:

- hiding FRI: salted Keccak-256 Merkle commitments and random codewords, so that the openings
  reveal nothing about the witness (zero knowledge);
- blowup 8, 38 queries, 16 bits of proof of work before the queries: 130 bits of conjectured
  security;
- the prover draws fresh randomness for every proof; two proofs of the same transaction differ.

A proof is about 180 kB. On an Apple M1, proving takes about 0.1 s; verifying takes about 13 ms,
natively as in the WebAssembly runtime. Block length, not compute, bounds the number of private
transactions per block: about 20 with 5 MiB blocks.

## Note encryption

For each output, the sender encapsulates to the recipient's ML-KEM key, derives a key with
SHA3-256 and encrypts `v || rho || r` with ChaCha20-Poly1305. The ciphertext (1168 bytes: 1088 for the KEM, 64 for the note, 16 for the tag) is
published with the transaction. Wallets try to decrypt every new note with their decapsulation
key and check the decrypted note against its commitment.

## Chain rules

The rules are implemented by `pallet-shielded` (`pallets/shielded`).

- Nullifiers are stored; a transaction spending a known nullifier, or the same nullifier twice,
  is rejected.
- The anchor must be one of the last 100 roots. The chain records one root per block that
  changes the tree, the root at the end of the block, so that several transactions of a block
  can use the same anchor.
- Turnstile: the chain tracks the total value in the pool. `value_in` adds to it, `value_out` and
  `fee` are taken from it, and it can never become negative. A flaw in the proof system could not
  create TESS outside the pool.
- Public amounts and fees must be below `2^60` planck, as note values: a single note holds at
  most 1 152 921 TESS.

Two calls:

- `shield`, signed by the account paying `value_in`. The binding hash covers the signer, so that
  the proof cannot be replayed from another account. The signer pays the usual transaction fee.
- `transact`, submitted without signer (a general transaction authorised by its proof), so that
  it is linked to no account. `value_in` must be zero. Its `fee` is paid from the pool to the
  block author and must be at least what a signed transaction of the same weight and length pays.
  The proof and every other rule are checked when the transaction enters the pool and again when
  the block is imported; once authorised, the call cannot fail, so that every included
  transaction pays its fee. The transaction pool rejects conflicting transactions through their
  nullifiers.

Value moved into the pool is burned from the transparent supply and value moved out is minted:
the total supply is the total issuance of the balances pallet plus the pool balance.

The binding hash is `blake2_256("tesserax-shielded-binding" || source || recipient || notes)`
(SCALE encoded), reduced to a digest. Wallets compute it before proving.

Each applied transaction emits `Transacted` with the nullifiers, the commitments, the position of
the first new note and both encrypted notes: wallets rebuild the note tree from these events and
try to decrypt every note.
