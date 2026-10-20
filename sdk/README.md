# tesserax-sdk

Rust SDK of Tesserax: ML-DSA-87 keys and addresses, balances and transfers, delegations from an
owner to an agent key, and the agent registry with its reputation.

```rust
use tesserax_sdk::{Amount, Client, Keypair, Permissions, Policy, Profile};

let client = Client::connect("ws://127.0.0.1:9944").await?;

// An owner delegates 10 TESS a day, at most 2 per payment, to an agent key.
let owner = Keypair::from_uri("<24-word phrase>")?;
let (agent, agent_phrase) = Keypair::generate()?;
client.transfer(&owner, &agent.account_id(), Amount::from_tess("0.5")?).await?; // agent fees
client
	.authorize_agent(&owner, &agent.account_id(), &Policy {
		budget: Amount::from_tess("10")?,
		period_blocks: 4_800,
		max_per_call: Amount::from_tess("2")?,
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: false },
		targets: vec![],
	})
	.await?;

// The agent finds a translator and pays it from the owner's funds.
let translators = client.find_agents(Some("translation"), true).await?;
let payee = tesserax_sdk::parse_address(&translators[0].address)?;
client.agent_pay_agent(&agent, &owner.account_id(), &payee, Amount::from_tess("0.25")?).await?;
```

Micropayments (`docs/micropayments.md`): `ChannelPayer` pays HTTP 402 services with vouchers on
payment channels it opens and tops up on demand, `ChannelReceiver` verifies the vouchers paying a
service and claims them. `with_file` keeps their state in a file, so that a restart loses
neither channels nor vouchers.

```rust
// A service charging 0.001 TESS per request, keeping its vouchers across restarts.
let receiver = ChannelReceiver::with_file(client.clone(), service_key, "service-channels.json")?;
match request.header(VOUCHER_HEADER) {
	Some(voucher) => match receiver.verify(voucher, price).await {
		Ok(_) => serve(),
		Err(rejection) => respond_402(receiver.payment_required(price, Some(&rejection))),
	},
	None => respond_402(receiver.payment_required(price, None)),
}
// From time to time:
receiver.claim_all().await?;
```

Task market: `create_task`, `open_tasks`, `accept_task`, `deliver_task`, `approve_task`,
`dispute_task`, `resolve_task` and `finalize_task`; every call takes an optional owner to act
through an agent's delegation.

The client reads the state of the best block and builds transactions on top of it: proof-of-work
blocks are final only 60 blocks deep. Calls return once the transaction is included in a block,
or the error of the chain, such as `Agents::BudgetExceeded`.

Amounts are `Amount` values in planck, parsed from and serialized as decimal strings of TESS.
Addresses use the SS58 prefix 2261 and start with `te`.

## Metadata

The types and calls are generated from `metadata/tesserax.scale`. A test checks that it matches
the runtime; after a runtime change, update it with:

```bash
TESSERAX_UPDATE_METADATA=1 cargo test -p tesserax-sdk --test metadata
```

## Test

```bash
cargo test -p tesserax-sdk
tesserax --dev --tmp --mine --author <address>
cargo test -p tesserax-sdk --test dev_chain -- --ignored --nocapture
```
