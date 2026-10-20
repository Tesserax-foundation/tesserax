# Vendored forks

Tesserax's peer-to-peer layer is post-quantum: node identities are ML-DSA-65 keys and every
connection is secured with TLS 1.3 restricted to ML-KEM-768 key exchange and ML-DSA-65
authentication. The crates below are forks of the published Polkadot SDK crates, wired in through
`[patch.crates-io]` in the workspace manifest.

| Crate | Base version | License |
|---|---|---|
| `litep2p` | 0.15.3 | MIT |
| `pallet-revive` | 0.19.1 (polkadot-sdk stable2606) | Apache-2.0 |
| `sc-cli` | 0.64.0 (polkadot-sdk stable2606) | GPL-3.0-or-later WITH Classpath-exception-2.0 |
| `sc-network` | 0.61.0 (polkadot-sdk stable2606) | GPL-3.0-or-later WITH Classpath-exception-2.0 |
| `sc-network-types` | 0.23.3 (polkadot-sdk stable2606) | Apache-2.0 |
| `sc-telemetry` | 33.0.0 (polkadot-sdk stable2606) | GPL-3.0-or-later WITH Classpath-exception-2.0 |

## litep2p

- Node identities are ML-DSA-65 keys (`crypto::ml_dsa`, backed by AWS-LC through `aws-lc-rs`).
  The `PeerId` is the SHA-256 multihash of the protobuf encoded public key. Ed25519 and RSA
  identities are removed.
- The Noise handshake (X25519) is replaced by TLS 1.3 (`crypto::tls`, `rustls` with the
  `aws-lc-rs` provider), negotiated as `/tesserax/tls/1.0.0` on TCP and WebSocket connections:
  - key exchange: `MLKEM768` only, no elliptic curve or hybrid group;
  - authentication: mutual, ML-DSA-65 raw public keys (RFC 7250), no X.509 certificates;
  - cipher suites: `TLS13_AES_256_GCM_SHA384`, `TLS13_CHACHA20_POLY1305_SHA256`;
  - no session resumption, no early data, no TLS 1.2.
- The QUIC and WebRTC transports are removed: both require elliptic curve cryptography.
- A system DNS configuration with name servers the resolver cannot parse (scoped IPv6 link-local
  addresses such as `fe80::1%en0`, advertised by some routers) no longer prevents the node from
  starting: the valid name servers of `/etc/resolv.conf` are used, or the default resolver.

## sc-network

- The libp2p network backend is removed (swarm, Noise, Kademlia, mDNS, identify, ping,
  request-response and notification behaviours) together with the `libp2p` dependency. litep2p is
  the only backend.
- `NodeKeyConfig` holds the 32 byte seed of the node's ML-DSA-65 identity key.
- WebRTC support is removed. Default `webrtc-direct` listen addresses are dropped and public
  `webrtc-direct` addresses are rejected.

## sc-cli

- `--node-key-type` only accepts `ml-dsa`. The node key file is `network/secret_ml_dsa` in the
  chain directory; `--node-key` and `--node-key-file` take the hex encoded 32 byte seed.
- `generate-node-key` and `inspect-node-key` generate and inspect ML-DSA-65 node keys.
- `--network-backend` only accepts `litep2p`; WebRTC listen addresses are no longer added by
  default and `--force-enable-webrtc` is removed.

## sc-network-types

- The Ed25519 module and the libp2p Kademlia conversions are removed. `libp2p-identity` is only
  used for the `PeerId` type of multiaddresses, without any key algorithm.

## sc-telemetry

- The libp2p WebSocket transport is removed: telemetry would be sent over classical TLS. The
  public API is unchanged, telemetry messages are dropped and configured endpoints are reported
  once in the logs.

## pallet-revive

- The builtin precompiles based on elliptic curves are removed: `ecrecover` (secp256k1),
  `bn128` add, mul and pairing, `p256_verify` (secp256r1) and KZG `point_eval`.
- The contract API helpers `sr25519_verify`, `ecdsa_recover` and `ecdsa_to_eth_address` always
  fail.
- The benchmarks of the removed precompiles and of `ecdsa_to_eth_address` are removed.
- EVM bytecode and Ethereum transactions are disabled by the runtime configuration
  (`AllowEVMBytecode = false`, Ethereum calls filtered).
