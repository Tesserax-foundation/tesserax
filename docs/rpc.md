# Public RPC

The public RPC of the devnet:

```
wss://rpc.tesserax.app:21207      websocket (subscriptions)
https://rpc.tesserax.app:21207    HTTP, one JSON-RPC request per call
```

The port is the public port of the TCP proxy of the host (Railway), which hands the connection to
the gateway untouched: TLS is terminated by the gateway itself, not by the host.

## Post-quantum TLS

`tesserax-rpc-gateway` (`rpc-gateway/`) accepts TLS 1.3 only, and only key exchanges with
ML-KEM-768:

| Client offers | Key exchange |
|---|---|
| `MLKEM768` | ML-KEM-768 alone |
| `X25519MLKEM768` only (every browser) | X25519 and ML-KEM-768, hybrid |
| classical groups only (`X25519`, `P-256`, ...) | refused: handshake failure |

No session is protected by classical cryptography alone. Browsers offer ML-KEM only within the
hybrid X25519MLKEM768, so the gateway accepts the hybrid; clients that offer ML-KEM-768 on its own
get it. The certificate is ECDSA, from Let's Encrypt: certificate authorities trusted by browsers
issue no post-quantum certificates yet.

Clients that connect:

- Chrome and Edge 131+, Firefox 132+, Safari on macOS and iOS 26: the web wallet and the
  extension.
- The desktop wallet: WebView2 (Windows), WebKit on macOS 26.
- Node.js, Python and curl built with OpenSSL 3.5 or later.
- Rust with rustls 0.23.27 or later and the `aws-lc-rs` provider.

## Limits

The gateway relays to an RPC endpoint of the node that serves the safe methods only, with:

- 600 calls per minute per connection;
- 1,000 connections at once on the node, 4,096 on the gateway;
- 64 subscriptions per connection, 50 calls per batch, 10 MB per request and per response.

The TCP proxy does not pass on the address of the client: limits apply per connection.

## Running a gateway

```bash
VERCEL_TOKEN=<token> tesserax-rpc-gateway \
  --domain rpc.example.com --dns-zone example.com \
  --upstream 127.0.0.1:9945 --state /var/lib/tesserax-rpc-gateway
```

- `--upstream`: an RPC endpoint of the node with the safe methods and a rate limit, such as
  `--experimental-rpc-endpoint listen-addr=127.0.0.1:9945,methods=safe,rate-limit=600`.
- Certificate: obtained from Let's Encrypt with DNS-01 challenges, written in the zone through the
  Vercel API (`VERCEL_TOKEN`, and `--vercel-team` when the zone belongs to a team), and renewed
  once two thirds of its lifetime have passed. The state directory keeps the ACME account, the
  certificate and its key. `--certificate` and `--key` serve PEM files instead.
- `--listen` (`[::]:8443` by default), `--max-connections`, `--handshake-timeout`: see
  `tesserax-rpc-gateway --help`. Every option can be given as an environment variable, such as
  `DOMAIN` or `UPSTREAM`.
- `docker build -f rpc-gateway/Dockerfile .` builds its image.
