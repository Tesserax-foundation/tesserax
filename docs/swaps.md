# Swaps

TESS trade against BTC, and against ETH, USDC and USDT on Ethereum, Base and Arbitrum, with atomic
swaps: nobody,
neither the foundation nor an exchange, holds the funds of another. Each side is locked on its own
chain by a hash time-locked contract, and both are released by the same secret: either both
parties are paid, or both get their funds back.

## How a swap goes

A **maker** sells TESS with an offer; a **taker** buys them.

1. The maker offers TESS on Tesserax (`pallet-swaps`): an amount held from its account, a price in
   the smallest unit of the asset for one TESS, the chain and asset it wants, and its EVM address.
2. The taker picks a random secret, and locks the payment for the maker in the contract
   `TesseraxSwap` of the chain, under the SHA-256 hash of the secret, for **24 hours**. The lock
   names the offer and the Tesserax account of the taker.
3. The maker checks the lock: the asset and the amount the price asks, its address, the hash, and
   at least 12 hours left. It then fills the offer: the TESS are locked on Tesserax for the taker
   under the same hash, for **6 hours**.
4. The taker claims the TESS with the secret. The claim needs no signature and no fee: the secret
   authorizes it, so that a taker who has no TESS yet can make it.
5. The secret is now public: anyone submits it to the contract, which pays the maker. The maker
   does it itself, within the 18 hours its payment remains locked at least.

If the maker never fills, the taker takes its payment back once the 24 hours are over. If the
taker never claims, the TESS return to the offer after 6 hours, and the taker takes its payment
back after 24. The chain of Tesserax cannot see the other chains: the maker checks the payment
before filling. A maker filling wrongly only exposes its own TESS, and a taker never claims a
trade that does not match its payment.

## Chains and assets

| Chain | Assets | Chain id | Testnet of the Tesserax testnet |
|---|---|---|---|
| Bitcoin | BTC | | Signet |
| Ethereum | ETH, USDC, USDT | 1 | Sepolia (ETH, USDC) |
| Base | ETH, USDC | 8453 | Base Sepolia (ETH, USDC) |
| Arbitrum | ETH, USDC, USDT | 42161 | Arbitrum Sepolia (ETH, USDC) |

Base and Arbitrum cost cents a transaction, Ethereum dollars. Token contracts are those of
Circle for USDC and of Tether for USDT (USD₮0 on Arbitrum); tokens taking a fee on transfer are
refused. Monero has no scripts, so no atomic swap is possible against it without elliptic curve
cryptography on Tesserax: holders of XMR swap them for BTC first, with the existing XMR–BTC atomic
swaps, then BTC for TESS.

`TesseraxSwap` is at `0xeA2f4f08D2A0f1972031B98De63f4B0395c256Fc` on every chain, deployed
with the deterministic CREATE2 deployer ([swaps/evm](../swaps/evm/README.md)).

### Bitcoin

Bitcoin has no contract: the payment is a P2WSH output of a script the taker writes, which pays
the maker with the secret, or the taker after the timelock (24 hours, compared to the median
time of the last blocks):

```text
OP_IF
    OP_SIZE 32 OP_EQUALVERIFY OP_SHA256 <hash> OP_EQUALVERIFY <maker key>
OP_ELSE
    <timelock> OP_CHECKLOCKTIMEVERIFY OP_DROP <taker key>
OP_ENDIF
OP_CHECKSIG
```

The payout of a Bitcoin offer is the compressed public key of the maker. The transaction locking
a payment also sends 330 satoshis to the P2WPKH address of that key, and carries an OP_RETURN
output of 78 bytes: `TX`, the offer, the Tesserax account of the taker, the hash and the
timelock. The maker watches its own address only; the refund key of the taker is the key of the
first input. The maker rebuilds the script, checks it is the one of the output, and fills once
the transaction has 2 confirmations: an unconfirmed transaction can be replaced. The payment lock
on Tesserax is the outpoint of the script, 36 bytes. The maker claims with the secret the taker
revealed, to the same address.

## Using it

In the web wallet, the **Market** page: buyers pick an offer and pay from their browser EVM
wallet (MetaMask, Rabby or any EIP-1193 wallet), whose keys never reach Tesserax; the wallet
claims the TESS as soon as the maker fills, and takes the payment back if it never does. Sellers
create offers there and run the maker of their account, which fills them while the wallet is
open and lists the payments to collect.

With the command line wallet, which runs a maker without a browser and pays in BTC; the EVM key
is a file holding a private key in hexadecimal, which pays the locks and the gas, and the Bitcoin
key a file holding a private key in WIF, which pays from its P2WPKH address:

```bash
# Makers: offer 10,000 TESS at 0.05 USDC on Base, paid to an EVM address.
tesserax-wallet swap offer alice --chain base --asset usdc --price 0.05 --amount 10000 \
  --min 100 --payout 0x...
# Run the maker: it fills the offer as payments come, collects them, takes back expired trades.
tesserax-wallet swap make alice --evm-key-file maker.key
tesserax-wallet swap cancel alice 3

# Takers: list the offers, buy 500 TESS of offer 3. The account receiving them needs no TESS.
tesserax-wallet swap offers
tesserax-wallet swap buy bob 3 500 --evm-key-file taker.key
# If the wait was interrupted, or the maker never filled:
tesserax-wallet swap claim bob 0x<lock>
tesserax-wallet swap refund-payment 0x<lock> --evm-key-file taker.key

# In BTC: the offer is paid to the key of maker.wif, the buy from the address of taker.wif.
tesserax-wallet swap offer alice --chain bitcoin --asset btc --price 0.000002 --amount 10000 \
  --btc-key-file maker.wif
tesserax-wallet swap make alice --btc-key-file maker.wif --evm-key-file maker.key
tesserax-wallet swap buy bob 4 500 --btc-key-file taker.wif
tesserax-wallet swap refund-payment <txid>:<vout> --btc-key-file taker.wif
```

Bitcoin is read and written through the Esplora API of mempool.space; `--esplora` gives another.

The secret of a buy is saved in the wallet directory before the payment is locked, and deleted
once the TESS are claimed: losing it before the claim only delays the refund. A buy in BTC also
saves its script, which takes the payment back. The Rust SDK has the
same operations: `tesserax_sdk::swaps`, and `tesserax_sdk::swaps::evm` with the `swaps` feature;
the TypeScript SDK `client.swaps` and `EvmSwaps`, with any viem wallet client. Both refuse to lock
a payment on a chain where the contract is not deployed.

## Market data

The explorer serves the prices, volumes, order book and trades of the market for price
aggregators, in the format of CoinGecko, and the supply of TESS: `/api/coingecko/tickers`,
`/api/coingecko/orderbook?ticker_id=TESS_USDC`, `/api/supply/circulating` and the others of
[explorer/README.md](../explorer/README.md#market-api). They count the trades their taker
claimed, the ones whose payment the maker can collect.

## Risks

- A maker must be online to fill: a taker whose payment is not filled waits 24 hours for its
  refund. Offers show how many trades a maker filled and completed.
- The taker holds an option for the time the TESS are locked, 6 hours: it may not claim if the
  price moved. Makers price it, and the short lock limits it.
- Blocks of Tesserax are final 60 blocks deep, about 18 minutes, well within the 6 hours of a
  trade; makers fill payments the EVM chain already included, and Bitcoin payments 2 blocks
  deep.
