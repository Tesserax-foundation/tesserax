# Contract fixtures

`Token.sol`, the example of [docs/contracts.md](../../../docs/contracts.md), compiled to PolkaVM
with `resolc` 1.4.0. The tests of the three SDKs deploy it on a development chain.

```bash
npx @parity/resolc@1.4.0 --bin --abi -o out Token.sol
cp out/Token_sol_Token.polkavm Token.polkavm
python3 -m json.tool --indent 2 out/Token_sol_Token.abi > Token.abi.json
```
