# Micropayments

AI agents pay per request: per API call, per inference, per page. A transaction per payment
would cost more than the payment and take a block. Tesserax uses payment channels
(`pallets/channels`): one transaction to open, payments as vouchers signed off chain, one
transaction to claim.

## Channels

1. The payer opens a channel to a payee with a deposit, held from its account, and names the
   ML-DSA-65 key that signs vouchers: its own key, or the key of an agent spending for it.
2. Each payment is a voucher carrying the cumulative amount owed on the channel. The payee
   checks it off chain in about a millisecond and serves.
3. The payee claims the latest voucher whenever it wants: the chain pays it the difference with
   what it already claimed. Older vouchers are worth less, so only the latest matters.
4. Closing:
   - the payee closes when it is done;
   - the payer requests to close; the payee has 6 hours (`ClosePeriod`, 1 200 blocks) to claim
     its latest voucher; then anyone finalizes the close.

   What was not claimed returns to the payer, with the storage deposit.

| Call | Who | Effect |
|---|---|---|
| `Channels::open(payee, signer, deposit)` | payer | Holds `deposit` and a storage deposit (about 0.24 TESS) |
| `Channels::top_up(id, amount)` | payer | Adds to the deposit, unless closing |
| `Channels::claim(id, amount, signature)` | payee | Pays `amount - claimed` from the deposit |
| `Channels::close(id)` | payee | Closes, refunds the payer |
| `Channels::request_close(id)` | payer | Starts the close period |
| `Channels::finalize_close(id)` | anyone | Closes once the close period is over |

The minimum deposit is 0.01 TESS. Opening costs one transaction; payments cost nothing; a claim
costs about 0.002 TESS of fees (it verifies an ML-DSA signature on chain), whatever the number of
payments it collects: a service claims when what it is owed is well above that.

An agent spending for an owner (`docs/agents.md`) opens and funds channels through its
delegation: the deposit counts in its budget and the payee must be one of its targets, if any.
The owner's funds are then exposed only up to the deposits, and the agent signs vouchers with its
own key.

## Vouchers

A voucher of cumulative `amount` (in planck) on channel `id` is the ML-DSA-65 signature, with
the FIPS 204 context `tesserax-channel`, of the SCALE encoding of

```text
(genesis_hash: [u8; 32], id: u64, amount: u128)
```

that is the 32 bytes of the genesis hash, then `id` in 8 bytes and `amount` in 16 bytes, little
endian. The genesis hash binds vouchers to one chain; the context keeps them apart from
transaction signatures.

## HTTP 402

A paid HTTP service answers a request without payment, or with an insufficient one, with
status `402 Payment Required` and the header

```text
Tesserax-Payment: {"payee":"te...","price":"0.001","genesis":"0x..."}
```

- `payee`: address of the service, payee of the channels;
- `price`: price of the request, in TESS;
- `genesis`: genesis hash of the chain;
- `accepted` (optional): cumulative amount the service already accepted on the channel of the
  rejected voucher, in TESS;
- `error` (optional): why the voucher was rejected.

The client retries the request with the header

```text
Tesserax-Voucher: {"channel":12,"amount":"0.037","signature":"<base64>"}
```

where `amount` is the new cumulative amount, at least the previous one plus the price, and
`signature` the base64 voucher signature. Amounts are decimal strings of TESS.

The service accepts a voucher if:

- the channel exists and its payee is the service;
- `amount` does not exceed the deposit;
- `amount` is at least the last amount it accepted on the channel plus the price;
- the signature is valid for the channel's signer;
- the channel is not closing, or closes far enough in the future for the service to claim.

It records the voucher as the latest of the channel and claims it on chain from time to time,
and before a closing channel closes.

A client sends one request at a time per channel: vouchers are cumulative, and a service
rejects a voucher below one it already accepted. A client that lost its count resumes from the
`accepted` amount of a rejection, bounded by the deposit.

The SDKs implement both sides:

| | Pay (client) | Get paid (service) |
|---|---|---|
| Rust `sdk/` | `ChannelPayer`, used by the MCP tool `tesserax_fetch` | `ChannelReceiver` |
| Python `sdk-python/` | `ChannelPayer.request` | `ChannelReceiver.verify` |
| TypeScript `sdk-ts/` | `ChannelPayer.fetch` | `ChannelReceiver.verify` |
