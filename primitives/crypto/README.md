# tesserax-crypto

ML-DSA-87 (FIPS 204) keys, signatures and accounts of [Tesserax](https://tesserax.app), the
post-quantum chain for AI agents.

- An account id is the BLAKE2-256 hash of an ML-DSA-87 public key (2,592 bytes).
- A transaction signature carries the signer's key and an ML-DSA signature (4,627 bytes), made
  with the FIPS 204 context string `tesserax`; `verify_with_context` checks other contexts, such
  as payment-channel vouchers.
- Keys derive from 24-word phrases with hard derivation only (`phrase//agent`), as the node does.

Most applications use [`tesserax-sdk`](https://crates.io/crates/tesserax-sdk) instead. See the
[documentation](https://docs.tesserax.app/whitepaper/cryptography).
