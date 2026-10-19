# RandomTESS

RandomTESS is the proof-of-work hash function of Tessera. It is RandomX v2 built with a Tessera
specific configuration, so that hashpower rented or pooled for other RandomX chains cannot be
pointed at Tessera without a dedicated miner.

## Sources

`randomx/` contains the RandomX sources from <https://github.com/tevador/RandomX> at commit
`a901d5bff16517ecf5c98c92c233e8480e259fc4`, under the BSD 3-Clause license (`randomx/LICENSE`).
The only modifications are the configuration values below, in `randomx/src/configuration.h` and
the generated `randomx/src/asm/configuration.asm`.

## Configuration

| Parameter | RandomX | RandomTESS |
|---|---|---|
| `RANDOMX_ARGON_SALT` | `"RandomX\x03"` | `"RandomTESS\x01"` |
| `RANDOMX_FREQ_IROR_R` | 8 | 6 |
| `RANDOMX_FREQ_IROL_R` | 2 | 4 |
| `RANDOMX_FREQ_FADD_M` | 5 | 4 |
| `RANDOMX_FREQ_FSUB_M` | 5 | 6 |

The instruction pairs IROR_R/IROL_R and FADD_M/FSUB_M are functionally equivalent; rebalancing
them is listed as safe in the RandomX configuration guide. All memory and program parameters are
unchanged: 256 MiB cache, 2080 MiB dataset, 384 instructions per program (v2), 2048 iterations,
8 programs per hash. Every VM is created with the RandomX v2 flag.

## Usage

```rust
use randomtess::{Cache, Flags, Vm};
use std::sync::Arc;

let flags = Flags::recommended();
let cache = Arc::new(Cache::new(flags, b"key")?);
let mut vm = Vm::light(flags, cache)?;
let hash = vm.hash(b"input");
```

Light mode (cache only) is used to verify blocks, fast mode (`Dataset` + `Vm::fast`) to mine.

## Tests

`cargo test -p randomtess` checks determinism, JIT/interpreter agreement and reference vectors.
`cargo test -p randomtess -- --ignored` also checks that fast and light modes agree; it allocates
the full dataset.
