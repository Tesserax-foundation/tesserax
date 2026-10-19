# Contract fixtures

`Counter.polkavm` is `Counter.sol` compiled to PolkaVM with `resolc` 1.4.0:

```bash
npx @parity/resolc@1.4.0 --bin -o out Counter.sol
cp out/Counter_sol_Counter.polkavm Counter.polkavm
```
