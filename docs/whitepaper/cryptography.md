---
title: "3. Cryptography"
description: "ML-DSA-87 accounts and signatures, key derivation, post-quantum transport for peers and the public RPC, and the hash functions of the chain."
---

# 3. Cryptography

Tesserax uses three families of primitives, all believed secure against quantum adversaries:

| Purpose | Primitive | Standard | Hardness assumption |
|---|---|---|---|
| Signatures: accounts, transactions, vouchers, node identities | ML-DSA-87 | FIPS 204 | Module-LWE, Module-SIS (lattices) |
| Key exchange: peers, RPC, encrypted notes | ML-KEM-768 | FIPS 203 | Module-LWE (lattices) |
| Hashing, proofs, symmetric encryption | BLAKE2, SHA-2, SHA-3, Keccak, Poseidon2, ChaCha20-Poly1305, AES-256-GCM | — | Generic; Grover halves security at most |

No security property of the chain depends on the discrete logarithm or factoring problems.

## 3.1 ML-DSA-87

ML-DSA is the lattice-based signature scheme standardized by NIST in FIPS 204. Tesserax uses its
strongest parameter set, ML-DSA-87, at NIST security category 5 — comparable to AES-256 against
key search — rather than the category-3 ML-DSA-65 that most deployments choose: keys protect funds
for decades, and the cost difference is small next to the size the scheme already imposes.

| | ML-DSA-87 | Ed25519 (for comparison) |
|---|---|---|
| Public key | 2,592 bytes | 32 bytes |
| Signature | 4,627 bytes | 64 bytes |
| Seed (private material) | 32 bytes | 32 bytes |
| Verification (Apple M1, WebAssembly runtime) | about 0.86 ms | tens of microseconds |

The runtime uses the RustCrypto `ml-dsa` implementation; the network layer uses AWS-LC; the
TypeScript SDK uses `@noble/post-quantum` and the Python SDK `dilithium-py`. Shared test vectors
make every implementation derive the same keys and accept the same signatures.

### Accounts

An account identifier is the hash of the public key:

$$
\text{AccountId} = \text{BLAKE2-256}(pk), \qquad |pk| = 2{,}592
$$

Hashing keeps identifiers at 32 bytes and keeps the key itself off chain until the account first
signs. Addresses encode the identifier in SS58 with network prefix 2261, so that every Tesserax
address starts with `te`, and addresses of other networks are rejected.

### Transactions

Because the identifier does not reveal the key, a transaction carries both:

$$
\text{signature} = (pk, \sigma), \qquad |pk| + |\sigma| = 2{,}592 + 4{,}627 = 7{,}219 \text{ bytes}
$$

A transaction is valid only if $\text{BLAKE2-256}(pk)$ equals the sender and $\sigma$ verifies
under $pk$. Every signature uses the FIPS 204 **context string** mechanism, which prefixes the
message with the context before signing:

$$
M' = 0 \;\Vert\; |\text{ctx}| \;\Vert\; \text{ctx} \;\Vert\; M
$$

| Context | Signs |
|---|---|
| `tesserax` | Transactions |
| `tesserax-channel` | Payment-channel vouchers |
| Chosen by the application | Messages verified by contracts through the ML-DSA precompile |

A signature made for one context never verifies in another: a voucher cannot be replayed as a
transaction, and contracts can never be shown a transaction signature as if it were theirs — the
precompile refuses the context `tesserax` outright. Signatures are hedged: they mix fresh
randomness into the signing nonce when it is available, which protects against fault attacks and
against nonce reuse in faulty random generators, and fall back to the deterministic variant of
FIPS 204 otherwise.

There is no other account type. The runtime's signature type accepts ML-DSA only; there is no
`MultiSignature` that could admit sr25519, ed25519 or ECDSA. There are no session keys, since
proof of work needs none, and no block signatures: a block is authenticated by its work.

## 3.2 Keys

### From phrase to key

A key is generated from 32 bytes of operating-system randomness, presented as a 24-word BIP39
phrase (256 bits of entropy). Turning a phrase into a key follows the Substrate convention:

$$
\text{seed} = \text{PBKDF2-HMAC-SHA512}\big(\text{entropy},\ \texttt{"mnemonic"} \,\Vert\, \text{password},\ 2048\big)_{[0..32]}
$$

$$
(sk, pk) = \text{ML-DSA-87.KeyGen}(\xi = \text{seed})
$$

The PBKDF2 input is the phrase's entropy rather than its words, so a Tesserax phrase does not
yield the same seed as in BIP39 wallets of other chains: the phrase is portable between Tesserax
wallets, not across ecosystems. An optional password (`phrase///password`) changes the seed.

### Derivation

Only hard derivation exists. A path such as `phrase//agent//3` derives a new seed at each step:

$$
\text{seed}' = \text{BLAKE2-256}\big(\text{SCALE}(\texttt{"MlDsaHDKD"},\ \text{seed},\ \text{chain\_code})\big)
$$

where the chain code is the junction's SCALE encoding, zero-padded to 32 bytes or hashed if longer.
Soft derivation — deriving public keys without the private key — has no secure equivalent for
lattice signatures and is refused. One phrase can therefore hold an owner's key and the keys of
all its agents, each a separate account.

### Wallet storage

The command line wallet stores each account as the secret phrase encrypted with ChaCha20-Poly1305
under a key derived by Argon2id (64 MiB, 3 passes, 1 lane, 16-byte random salt), with the address
as associated data so that a file cannot be swapped between accounts; decryption re-derives the
address and checks it. The web wallet, extension and desktop application keep the same format, so
a file exported from one opens in the others. Unlocked keys stay in memory only.

## 3.3 Post-quantum transport

Signatures protect what is written on chain; the network carrying it must not be the weak link. A
classical key exchange recorded today can be decrypted later, revealing which peer sent which
transaction — and an active quantum attacker could impersonate peers. Tesserax replaces the
transport security of the Polkadot SDK.

### Peer to peer

The networking stack is litep2p, forked so that:

- **node identities are ML-DSA-87 keys.** A node's `PeerId` is the SHA-256 multihash of its
  public key; the identity key file stores its 32-byte seed;
- **connections use TLS 1.3 instead of Noise**, with:
  - key exchange **ML-KEM-768 only** — no elliptic curve, not even in a hybrid group;
  - authentication by **raw ML-DSA-87 public keys** (RFC 7250), mutual, without X.509 certificates
    or chains;
  - AES-256-GCM or ChaCha20-Poly1305;
  - no session resumption, no early data, no TLS 1.2;
- after the handshake, the negotiated group must be ML-KEM-768 and the presented key must hash to
  the expected `PeerId`, or the connection is dropped;
- only TCP and WebSocket transports remain; QUIC and WebRTC, which carry their own classical
  handshakes, are removed, as is the libp2p backend with its Noise protocol;
- telemetry, which would report to servers over classical TLS, is disabled.

A downgrade is impossible by construction: a node offers no classical group, so a peer that
supports none of its groups cannot connect at all.

### Public RPC

Wallets and applications reach the network through `wss://rpc.tesserax.app`, served by a
gateway that terminates TLS itself:

| The client offers | The gateway uses |
|---|---|
| ML-KEM-768 | ML-KEM-768 alone |
| X25519MLKEM768 (current browsers) | The hybrid of X25519 and ML-KEM-768 |
| Classical groups only | Nothing: the handshake fails |

Every session key of the RPC is thus protected by ML-KEM. Browsers support the hybrid group since
Chrome 131, Firefox 132 and Safari 26; Node.js, Python and curl support it with OpenSSL 3.5.
Transactions are signed with ML-DSA in the wallet before they reach the gateway, which cannot alter
them.

## 3.4 Hash functions

| Use | Function |
|---|---|
| Block hashes, extrinsic and state roots | BLAKE2b-256 |
| Account identifiers, key derivation | BLAKE2b-256 |
| Proof of work | RandomTESS, committed with BLAKE2b-256 |
| Storage keys of maps | BLAKE2-128 concatenated with the key |
| Atomic swaps | SHA-256, the hash Bitcoin scripts and EVM contracts can check |
| Contract addresses | Keccak-256, last 20 bytes |
| Network identities | SHA-256 |
| Shielded pool | Poseidon2 over the Goldilocks field inside proofs; Keccak-256 for proof commitments; SHA3 for note encryption keys |

Grover's algorithm gives at most a quadratic speed-up on preimage search, so 256-bit hashes keep
about 128 bits of security against quantum attackers; collision search gains less in practice. The
one place where 160 bits matter is contract addresses: section 8.3 explains why an address belongs
to its first owner, so that finding a second preimage of an account's address — about $2^{80}$
quantum operations — gains nothing.

## 3.5 What is removed

| Classical component of the Polkadot SDK | In Tesserax |
|---|---|
| sr25519, ed25519 and ECDSA accounts | Removed: ML-DSA-87 only |
| Ethereum-format transactions (ECDSA) | Filtered: rejected at the call filter |
| `ecrecover`, BN128, P-256 and KZG precompiles | Removed from contracts |
| `sr25519_verify`, `ecdsa_recover` contract helpers | Always fail |
| Ed25519 node identities, Noise handshake | ML-DSA-87 identities, TLS 1.3 with ML-KEM-768 |
| QUIC and WebRTC transports | Removed |
| BABE, Aura, GRANDPA and BEEFY | Not used: proof of work and depth finality |
| Session keys | None |
| Telemetry over classical TLS | Disabled |
