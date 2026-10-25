# Smart contracts

Contracts on Tesserax are written in Solidity, compiled to PolkaVM (RISC-V) with
[`resolc`](https://github.com/paritytech/revive) and executed by `pallet-revive`. Deploying and
calling them are ordinary transactions, signed with ML-DSA-87 like every other: there is no EVM,
no Ethereum transaction and no ECDSA anywhere.

The three SDKs (Rust, TypeScript, Python) deploy contracts, call them, read them without a
transaction and follow their events, with arguments and results encoded with the Solidity ABI:
functions are called by name, with values of the language.

## How it differs from Ethereum

| | Ethereum | Tesserax |
|---|---|---|
| Bytecode | EVM | PolkaVM, from `resolc`; EVM bytecode is rejected |
| Signatures | ECDSA (secp256k1) | ML-DSA-87; Ethereum transactions are rejected |
| JSON-RPC | `eth_call`, `eth_sendRawTransaction`, `eth_getLogs`... | the node's RPC: dry runs through `state_call`, transactions through `author_submitExtrinsic`, events in the blocks |
| Tooling | MetaMask, ethers.js, Hardhat, Foundry | the Tesserax SDKs and wallet |
| Precompiles | `ecrecover`, `bn128`, `p256`, KZG... | ML-DSA-87 verification; the elliptic curve ones are removed |

Solidity itself is unchanged: the same language, ABI and events. What changes is how a contract is
reached, and what a contract can no longer do: verify an ECDSA signature (`ecrecover` always fails).

## Addresses

A contract has a 20-byte address (`0x` and 40 hexadecimal digits). Every Tesserax account (`te...`)
also has one, its `msg.sender` in contracts and what `address` arguments name: the last 20 bytes of
the Keccak-256 hash of its account id. The runtime maps it when the account is created; the SDKs
compute it with `h160` (Rust), `toH160` (TypeScript) and `to_h160` (Python).

```
te5Gr...  (account, SS58)  ->  0x8e2a...c4f1  (its address in contracts)
```

Solidity fixes addresses at 160 bits, fewer than the 256 bits of accounts. Two keys whose
addresses collide can be found with about 2^80 hash computations, within reach of a large
attacker, so an address is owned by one account or one contract only, the first one:

- an account acts in contracts only with an address mapped to it; a second account with the
  same address cannot call or deploy contracts (`Revive::AccountUnmapped`);
- no contract is created at the address of an account (`Revive::DuplicateContract`), and no
  account is mapped to the address of a contract.

Taking the address of a given account then means finding a second key for that very address: a
second preimage of 160 bits, about 2^160 computations, and about 2^80 for a quantum computer
running Grover's algorithm sequentially. The account itself, its balance and its transactions
stay protected by ML-DSA-87 and 256-bit hashes.

Amounts sent to a contract (`value`) are in TESS, as everywhere on the chain. Inside the contract,
`msg.value` and balances count in 18 decimals, as Solidity code expects: 1 TESS is `1e18` there.

## Example

A token, the one the tests of the SDKs deploy
([sdk/fixtures/contracts/Token.sol](../sdk/fixtures/contracts/Token.sol)):

```solidity
// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

contract Token {
    string public name;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;

    event Transfer(address indexed from, address indexed to, uint256 value);

    error InsufficientBalance(uint256 available, uint256 required);

    constructor(string memory name_, uint256 supply) {
        name = name_;
        totalSupply = supply;
        balanceOf[msg.sender] = supply;
        emit Transfer(address(0), msg.sender, supply);
    }

    function transfer(address to, uint256 value) external returns (bool) {
        uint256 available = balanceOf[msg.sender];
        if (available < value) revert InsufficientBalance(available, value);
        balanceOf[msg.sender] = available - value;
        balanceOf[to] += value;
        emit Transfer(msg.sender, to, value);
        return true;
    }

    function burn(uint256 value) external {
        require(balanceOf[msg.sender] >= value, "burn exceeds balance");
        balanceOf[msg.sender] -= value;
        totalSupply -= value;
        emit Transfer(msg.sender, address(0), value);
    }
}
```

### Compile

```bash
npx @parity/resolc@1.4.0 --bin --abi -o out Token.sol
```

`out/Token_sol_Token.polkavm` is the code to deploy, `out/Token_sol_Token.abi` its ABI.

### TypeScript

```ts
import { readFileSync } from "node:fs";
import { ContractReverted, Keypair, TesseraxClient, toH160 } from "@tesserax/sdk";

const client = new TesseraxClient("wss://rpc.tesserax.app:21207");
const alice = Keypair.fromUri(process.env.TESSERAX_KEY!);
const bob = "te...";

const code = new Uint8Array(readFileSync("out/Token_sol_Token.polkavm"));
const abi = JSON.parse(readFileSync("out/Token_sol_Token.abi", "utf8"));

// Deploy: the constructor's arguments, then the contract once a block includes it.
const token = await client.deployContract(alice, { code, abi, args: ["Tesserax Coin", 1_000_000n] });
console.log(token.address);

// Read, without a transaction or a fee.
const balance: bigint = await token.read("balanceOf", [toH160(alice.address)]);

// Write: a transaction signed with ML-DSA-87, and the events it emitted.
const sent = await token.write(alice, "transfer", [toH160(bob), 250n]);
console.log(sent.contractEvents[0].args); // { from: "0x...", to: "0x...", value: 250n }

// A call that would revert is stopped before it costs a fee, with its reason.
try {
  await token.write(alice, "transfer", [toH160(bob), 10n ** 30n]);
} catch (error) {
  if (error instanceof ContractReverted) console.log(error.reason); // InsufficientBalance(available: ..., required: ...)
}

// Events: past blocks, and new ones as they come.
const past = await token.events(token.deployment.blockNumber, sent.blockNumber);
const stop = token.watchEvents((event) => console.log(event.name, event.args));

// A contract deployed earlier, from its address and ABI.
const existing = client.contract("0x...", abi);
```

Values follow `viem`: `uint`/`int` are `bigint`, `address` is a `0x` string, `bytes` a `0x`
string, tuples are objects or arrays.

### Python

```python
import json
from tesserax import Client, ContractReverted, Keypair, to_h160

client = Client("wss://rpc.tesserax.app:21207")
alice = Keypair.from_uri(open("alice.key").read())
bob = "te..."

code = open("out/Token_sol_Token.polkavm", "rb").read()
abi = json.load(open("out/Token_sol_Token.abi"))

token = client.deploy_contract(alice, code, abi, args=["Tesserax Coin", 1_000_000])
balance = token.read("balanceOf", [to_h160(alice.address)])

sent = token.write(alice, "transfer", [to_h160(bob), 250])
print(sent.contract_events[0].args)  # {'from': '0x...', 'to': '0x...', 'value': 250}

try:
    token.write(alice, "transfer", [to_h160(bob), 10**30])
except ContractReverted as error:
    print(error.reason)

past = token.events(token.deployment.block_number, sent.block_number)
for event in token.watch_events():  # blocks, yielding events as they come
    print(event.name, event.args)
```

Values follow `eth-abi`: integers are `int`, `address` a checksummed `0x` string, `bytes` are
`bytes`, tuples are tuples.

### Rust

```rust
use tesserax_sdk::{contracts::{h160, DynSolValue, JsonAbi, U256}, Client, Error, Keypair};

let client = Client::connect("wss://rpc.tesserax.app:21207").await?;
let alice = Keypair::from_uri(&std::fs::read_to_string("alice.key")?)?;
let bob = tesserax_sdk::parse_address("te...")?;

let code = std::fs::read("out/Token_sol_Token.polkavm")?;
let abi: JsonAbi = serde_json::from_str(&std::fs::read_to_string("out/Token_sol_Token.abi")?)?;
let uint = |n: u64| DynSolValue::Uint(U256::from(n), 256);

let args = [DynSolValue::String("Tesserax Coin".into()), uint(1_000_000)];
let (token, deployment) = client.deploy_contract(&alice, &code, abi, &args, Default::default()).await?;

let balance = token.read("balanceOf", &[DynSolValue::Address(h160(&alice.account_id()))]).await?;
let sent = token.write(&alice, "transfer", &[DynSolValue::Address(h160(&bob)), uint(250)]).await?;
for event in &sent.events {
    println!("{:?} {:?}", event.name, event.args);
}

match token.write(&alice, "burn", &[uint(u64::MAX)]).await {
    Err(Error::ContractReverted(reason)) => println!("{reason}"), // burn exceeds balance
    other => { other?; }
}

let past = token.events(deployment.outcome.block_number, sent.outcome.block_number).await?;
let mut stream = Box::pin(token.watch_events().await?);
while let Some(event) = futures::StreamExt::next(&mut stream).await {
    println!("{:?}", event?.args);
}
```

Values are `alloy` `DynSolValue`s. A contract can also be called with input encoded elsewhere, such
as by the `sol!` macro of `alloy`: `Client::call_contract` takes the encoded input,
`Client::dry_run_call` returns the raw output.

## In the wallet

The Contracts page of the wallet (web, extension and desktop) deploys and calls contracts without
code:

- **Deploy**: the `.polkavm` file of `resolc --bin` and the ABI; the constructor gets a field per
  argument. The review shows the exact fee, or the revert reason of the constructor before
  anything is signed.
- **Add**: a contract deployed by others, from its address and its ABI.
- **Read**: every `view` and `pure` function, called as a dry run from the address of the active
  account, without fee; functions without arguments are read when the page opens.
- **Write**: the other functions, with the TESS sent to `payable` ones, through the review and
  signature of every transaction.
- **Events**: those of the calls made in the wallet, and the latest ones from the explorer
  database when the network has one.

Fields of type `address` take `0x` addresses and Tesserax addresses (`te...`), mapped to their
address in contracts; integers are decimal or `0x`, arrays and tuples JSON. Contracts are listed
per network, in the browser.

## Reads, writes and fees

- **Reads** are dry runs: the node executes the call on the state of the best block and returns its
  result, with no transaction, no signature and no fee. The caller (`msg.sender`) is the zero
  account unless one is given (`from` in TypeScript, `sender` in Python, `read_as` in Rust).
- **Writes** are dry run first. The dry run gives the weight and the storage deposit the call needs:
  the transaction carries them, plus a 10% margin, as its limits. A call that would revert is
  stopped there, with its reason, before it costs a fee. The fee is that of any transaction: its
  weight and its length, paid to the miner.
- **Storage deposit**: storage a contract occupies is paid by a deposit, held from the caller and
  returned when the storage is freed.
- **Revert reasons** are decoded: `require` messages (`Error(string)`), panics (`Panic(uint256)`,
  such as an arithmetic overflow) and the errors the ABI declares, with their arguments.

## Events

The events of a contract are in the blocks that include its transactions. The SDKs read them for a
range of blocks (at most 10,000 at once; the node must keep the state of these blocks, as an
archive node such as the public RPC does) and follow new blocks as they come.

They follow the best chain. A proof-of-work block may still leave the chain, with its events, until
it is 60 blocks deep (about 18 minutes): an application that acts on an event for value waits until
then, as for a payment.

## ML-DSA signatures in contracts

Contracts verify ML-DSA-87 signatures through the precompile at
`0x00000000000000000000000000000000d5a00000`:

```solidity
interface IMlDsa87 {
    function verify(
        bytes calldata publicKey,
        bytes calldata context,
        bytes calldata message,
        bytes calldata signature
    ) external view returns (bool);
}
```

The FIPS 204 context `"tesserax"` is reserved for transactions and always fails, so that no
signature made for a contract can be replayed as a transaction, or the reverse.
