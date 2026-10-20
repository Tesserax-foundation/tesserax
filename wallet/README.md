# tesserax-wallet

Command line wallet of Tesserax: ML-DSA-87 accounts, transfers, delegations to AI agents, the
agent registry, payment channels and the task market.

```bash
cargo build --release -p tesserax-wallet
```

The binary is `target/release/tesserax-wallet`. It talks to a node at `ws://127.0.0.1:9944`, or
`--url` / `TESSERAX_URL`.

## Accounts

```bash
tesserax-wallet account new main          # a new 24-word phrase, shown once
tesserax-wallet account import backup     # from a phrase typed without echo, or piped
tesserax-wallet account list
tesserax-wallet account show main
tesserax-wallet account export main       # prints the phrase, after the password
tesserax-wallet account remove main
```

Accounts live in `~/.tesserax/wallet` (or `--wallet-dir` / `TESSERAX_WALLET_DIR`), one file per
account, readable by the user only. The phrase is encrypted with ChaCha20-Poly1305 under a key
derived from the password with Argon2id (64 MiB, 3 passes); the address is authenticated with
it. Symmetric encryption with 256-bit keys stays secure against quantum computers.

Write down the phrase of every account and keep it offline: it is the only way to recover the
account if the wallet directory is lost.

Commands that send a transaction show what they do and ask for confirmation; `--yes` skips it.
`TESSERAX_PASSWORD` gives the password to scripts. `--json` prints results as JSON.

## Transfers

```bash
tesserax-wallet balance main
tesserax-wallet transfer --from main te5... 12.5
```

Wherever an address is expected, the name of an account of the wallet works too.

## AI agents

```bash
# Give an agent key 10 TESS a day, at most 2 per call, for 30 days.
tesserax-wallet agent authorize --from main te9... --budget 10 --max-per-call 2 --expires-in-hours 720
tesserax-wallet agent show main te9...
tesserax-wallet agent revoke --from main te9...

tesserax-wallet registry register --from agent --name translator --capability translation \
  --endpoint https://translator.example/mcp --owner main
tesserax-wallet registry confirm-owner --from main te9...
tesserax-wallet registry find --capability translation
tesserax-wallet registry show te9...
tesserax-wallet registry pay --from main te9... 0.5
tesserax-wallet registry rate --from main te9... good
```

`--owner` makes an agent account act for its owner through its delegation: the chain checks the
budget and the limits.

## Payment channels

```bash
tesserax-wallet channel open --from main te9... 5
tesserax-wallet channel list main
tesserax-wallet channel top-up --from main 3 2
tesserax-wallet channel close --from main 3             # the rest returns after 6 hours
tesserax-wallet channel finalize --from main 3
# As a payee:
tesserax-wallet channel claim --from service '<Tesserax-Voucher header>'
tesserax-wallet channel close-as-payee --from service 3
```

## Tasks

```bash
tesserax-wallet task list --capability translation
tesserax-wallet task create --from main --spec https://example.com/job.md --capability translation \
  --reward 20 --delivery-hours 48 --review-hours 24
tesserax-wallet task accept --from worker 7
tesserax-wallet task deliver --from worker 7 ipfs://result
tesserax-wallet task approve --from main 7
tesserax-wallet task dispute --from main 7              # with an arbiter only
tesserax-wallet task finalize --from worker 7           # after the review period
```

## Test

```bash
cargo test -p tesserax-wallet
# Against a development chain (tesserax --dev --tmp --mine --author <address>):
cargo test -p tesserax-wallet --test dev_chain -- --ignored --nocapture
```
