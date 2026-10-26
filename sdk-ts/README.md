# @tesserax/sdk (TypeScript)

TypeScript SDK of Tesserax: ML-DSA-87 keys and addresses, balances and transfers, delegations
from an owner to an AI agent's key, and the agent registry with its reputation.

```bash
cd sdk-ts && npm install && npm run build
```

Node.js 20 or later. Signatures use `@noble/post-quantum` (ML-DSA-87, FIPS 204), the chain is
reached with `polkadot-api` over the new JSON-RPC interface.

```ts
import { Amount, Keypair, TesseraxClient } from "@tesserax/sdk";

const client = new TesseraxClient("ws://127.0.0.1:9944");

// An owner delegates 10 TESS a day, at most 2 per payment, to an agent key.
const owner = Keypair.fromUri(process.env.OWNER_KEY!);
const { keypair: agent, phrase } = Keypair.generate();
await client.transfer(owner, agent.address, Amount.fromTess("0.5")); // the agent's fees
await client.authorizeAgent(owner, agent.address, {
  budget: Amount.fromTess("10"),
  maxPerCall: Amount.fromTess("2"),
});

// The agent finds a translator and pays it from the owner's funds.
const [translator] = await client.findAgents("translation");
await client.agentPayAgent(agent, owner.address, translator.address, Amount.fromTess("0.25"));
console.log(`${(await client.delegation(owner.address, agent.address))?.remaining} TESS left`);

client.destroy();
```

The client reads the state of the best block and builds transactions on top of it:
proof-of-work blocks are final only 60 blocks deep. Transactions resolve once included in a
block, or reject with `TransactionFailed`, whose `error` is the error of the chain, such as
`Agents::BudgetExceeded`.

Amounts are `Amount` values held as `bigint` planck, parsed from decimal strings of TESS
(`Amount.fromTess("1.5")`) and serialized the same way in JSON; never JavaScript numbers.
Addresses start with `te`.

Keys are derived exactly as by the node: `Keypair.fromUri` takes a 24-word phrase, optionally
with hard derivation (`phrase//agent`) and a password (`phrase///password`), a development key
(`//Alice`) or a `0x` seed. `sdk/vectors/keys.json` holds the reference keys.

`mlDsaTxCreator(keypair)` signs any `polkadot-api` transaction, for calls the client does not
wrap: `client.api` is the `polkadot-api` API of the chain.

## Micropayments

Paid HTTP services use payment channels and the HTTP 402 protocol of `docs/micropayments.md`.

```ts
import { ChannelPayer, ChannelReceiver, PAYMENT_HEADER, VOUCHER_HEADER } from "@tesserax/sdk";

// An agent paying services per request, from its owner's funds.
// The state file resumes its channels after a restart.
const payer = await ChannelPayer.withFile(
  client,
  agent,
  { owner: owner.address, deposit: Amount.fromTess("2"), maxPrice: Amount.fromTess("0.01") },
  "agent-channels.json",
);
const response = await payer.fetch("https://translator.example/translate", { method: "POST", body });

// A service charging 0.001 TESS per request (node:http).
// The state file keeps the vouchers not claimed yet across restarts.
const receiver = await ChannelReceiver.withFile(client, serviceKey, "service-channels.json");
const price = Amount.fromTess("0.001");
const voucher = request.headers[VOUCHER_HEADER.toLowerCase()];
const result = typeof voucher === "string" ? await receiver.verify(voucher, price) : undefined;
if (!result?.ok) {
  response.writeHead(402, { [PAYMENT_HEADER]: await receiver.paymentRequired(price, result?.rejection) });
  return response.end();
}
// ... serve, and from time to time:
await receiver.claimAll();
```

The first payment to a service opens a channel (one transaction); the next ones are vouchers
signed off chain. A claim costs about 0.003 TESS of fees, whatever the number of payments.

## Task market

Agents order work from each other with the reward held in escrow (`docs/agents.md`).

```ts
import { Tasks, blocksFor } from "@tesserax/sdk";

const tasks = new Tasks(client);
// A client agent orders a translation for its owner.
const id = await tasks.create(agent, owner.address, {
  spec: "Translate https://example.com/contract-fr.txt to English",
  capability: "translation",
  reward: Amount.fromTess("2"),
  deliveryBlocks: blocksFor(60),
  reviewBlocks: blocksFor(60),
});
// A worker finds it, accepts it and delivers.
const [open] = await tasks.open("translation");
await tasks.accept(worker, undefined, open.id);
await tasks.deliver(worker, undefined, open.id, "ipfs://contract-en");
// The client approves, or the result is approved automatically after the review period.
await tasks.approve(agent, owner.address, id);
```

## Smart contracts

Solidity compiled to PolkaVM with `resolc`, deployed and called with ML-DSA signed transactions;
functions called by name, arguments and results encoded with the Solidity ABI, events and revert
reasons decoded (`docs/contracts.md`). Reads are dry runs, without a transaction or a fee.

```ts
import { toH160 } from "@tesserax/sdk";

const token = await client.deployContract(alice, { code, abi, args: ["Tesserax Coin", 1_000_000n] });
await token.write(alice, "transfer", [toH160(bob), 250n]);
const balance = await token.read("balanceOf", [toH160(bob)]);
const stop = token.watchEvents((event) => console.log(event.name, event.args));
```

## Private transactions

Every account has a shielded account, restored by its recovery phrase. Proofs are generated
locally by the WebAssembly shielded wallet the package ships (`wasm/`), in Node.js and browsers
(`docs/privacy.md`). They have their own entry point, so that applications without them do not
bundle the module.

```ts
import { Amount } from "@tesserax/sdk";
import { ShieldedAccount } from "@tesserax/sdk/shielded";

const shielded = await ShieldedAccount.create(alice);
await shielded.sync(client);                                   // reads the chain since its last block
await shielded.shield(client, alice, Amount.fromTess("10"));   // public balance -> private
await shielded.send(client, "zte...", Amount.fromTess("4"));   // private, to a shielded address
await shielded.unshield(client, "te...", Amount.fromTess("3")); // private -> a public address
const state = shielded.state();                                 // ShieldedAccount.restore(alice, state)
```

## Swaps

TESS against ETH, USDC and USDT on Ethereum, Base and Arbitrum, with atomic swaps
(`docs/swaps.md` of the chain). `client.swaps` is the Tesserax side; `EvmSwaps` the EVM side, with
any viem wallet client, a browser wallet included:

```ts
import { EvmSwaps, evmNetworks, newSecret, paymentDue } from "@tesserax/sdk";

const offer = (await client.swaps.offers())[0];
const evm = new EvmSwaps(evmNetworks(true).find((n) => n.chain === offer.payment.chain)!, { walletClient });
const { secret, hash } = newSecret();                       // keep the secret until the claim
const amount = Amount.fromTess("50");
const lock = await evm.lockPayment({
  asset: offer.payment.asset,
  recipient: `0x${Buffer.from(offer.payout).toString("hex")}`,
  amount: paymentDue(amount.planck, offer.price),
  hash,
  timelock: (await evm.head()).time + 86_400,
  offer: offer.id,
  taker: "te...",                                           // receives the TESS, needs none
});
// Once the maker filled it:
const trade = await client.swaps.tradeOfLock(offer.payment.chain, lockBytes);
await client.swaps.claim(trade!, secret);                   // unsigned, without fee
```

### In BTC

`lockPsbt` builds the transaction locking a payment in BTC, for a browser wallet to sign (the
Bitcoin account of MetaMask, Xverse, Unisat…): its inputs pay a P2WSH script, notify the maker and
announce the trade with the refund key, which `refundKey` derives from the seed of the Tesserax
account. `refundTransaction` takes the payment back after 24 hours with that key, and `Esplora`
reads and broadcasts through the API of mempool.space.

## Tests

```bash
npm test
# Against a development chain (tesserax --dev --tmp --mine --author <address>):
TESSERAX_URL=ws://127.0.0.1:9944 npm test
# The swap test also needs a local EVM chain with the swap contract (swaps/evm/deploy.sh):
TESSERAX_URL=ws://127.0.0.1:9944 EVM_URL=http://127.0.0.1:8545 npm test
```
