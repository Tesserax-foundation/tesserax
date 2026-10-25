# TesseraxSwap

The payment side of atomic swaps against TESS on the EVM chains: Ethereum, Base and Arbitrum, for
ETH, USDC and USDT. A taker locks its payment for the maker of a Tesserax offer under the SHA-256
hash of a secret; the claim of the TESS on Tesserax reveals the secret, which anyone then submits
here to pay the maker. Unclaimed, the payment returns to the taker after its timelock. See
[docs/swaps.md](../../docs/swaps.md).

```bash
forge test
```

Deployed with the deterministic CREATE2 deployer, the contract has the same address on every
chain: `0xeA2f4f08D2A0f1972031B98De63f4B0395c256Fc`, as long as the compiler and its settings of
`foundry.toml` are unchanged.

```bash
RPC_URL=https://mainnet.base.org ./deploy.sh --account deployer
```
