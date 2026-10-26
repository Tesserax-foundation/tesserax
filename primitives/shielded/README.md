# tesserax-shielded

Primitives of the shielded pool of [Tesserax](https://tesserax.app): notes, commitments,
nullifiers, the note tree, note encryption and the STARK of private transfers.

Everything is post-quantum: Poseidon2 over the Goldilocks field for commitments and the tree, a
Plonky3 STARK with hiding FRI for proofs, and ML-KEM-768 with ChaCha20-Poly1305 for note
encryption. There is no elliptic curve and no trusted setup.

Wallets use [`tesserax-shielded-wallet`](https://crates.io/crates/tesserax-shielded-wallet) or the
`shielded` feature of [`tesserax-sdk`](https://crates.io/crates/tesserax-sdk). See the
[documentation](https://docs.tesserax.app/whitepaper/privacy).
