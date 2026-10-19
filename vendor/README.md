# Vendored forks

Tessera's peer-to-peer layer is post-quantum: node identities are ML-DSA-65 keys and every
connection is secured with TLS 1.3 restricted to ML-KEM-768 key exchange and ML-DSA-65
authentication. The crates below are forks of the published Polkadot SDK crates, wired in through
`[patch.crates-io]` in the workspace manifest.

| Crate | Base version | License |
|---|---|---|
| `litep2p` | 0.15.3 | MIT |
| `sc-network` | 0.61.0 (polkadot-sdk stable2606) | GPL-3.0-or-later WITH Classpath-exception-2.0 |
| `sc-network-types` | 0.23.3 (polkadot-sdk stable2606) | Apache-2.0 |

## litep2p

- Node identities are ML-DSA-65 keys (`crypto::ml_dsa`, backed by AWS-LC through `aws-lc-rs`).
  The `PeerId` is the SHA-256 multihash of the protobuf encoded public key. Ed25519 and RSA
  identities are removed.
- The Noise handshake (X25519) is replaced by TLS 1.3 (`crypto::tls`, `rustls` with the
  `aws-lc-rs` provider), negotiated as `/tessera/tls/1.0.0` on TCP and WebSocket connections:
  - key exchange: `MLKEM768` only, no elliptic curve or hybrid group;
  - authentication: mutual, ML-DSA-65 raw public keys (RFC 7250), no X.509 certificates;
  - cipher suites: `TLS13_AES_256_GCM_SHA384`, `TLS13_CHACHA20_POLY1305_SHA256`;
  - no session resumption, no early data, no TLS 1.2.
- The QUIC and WebRTC transports are removed: both require elliptic curve cryptography.

## sc-network

- The litep2p backend derives its ML-DSA-65 identity from the 32 byte node secret, which is used
  as the FIPS 204 key generation seed.
- WebRTC support is removed. Default `webrtc-direct` listen addresses are dropped and public
  `webrtc-direct` addresses are rejected.

The libp2p backend is still compiled but the node refuses to start with it: only the litep2p
backend provides post-quantum connections.

## sc-network-types

- Conversions to the removed litep2p Ed25519 types are dropped. The node secret is generated as
  32 random bytes.
