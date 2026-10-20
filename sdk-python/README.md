# tesserax (Python SDK)

Python SDK of Tesserax: ML-DSA-87 keys and addresses, balances and transfers, delegations from an
owner to an AI agent's key, and the agent registry with its reputation.

```bash
pip install ./sdk-python
```

Python 3.10 or later. Signatures use `dilithium-py` (ML-DSA-87, FIPS 204), the chain is reached
with `substrate-interface`.

```python
from tesserax import Amount, Client, Keypair, Policy, Profile

client = Client("ws://127.0.0.1:9944")

# An owner delegates 10 TESS a day, at most 2 per payment, to an agent key.
owner = Keypair.from_uri(open("owner.key").read())
agent, agent_phrase = Keypair.generate()
client.transfer(owner, agent.address, Amount.from_tess("0.5"))  # the agent's fees
client.authorize_agent(owner, agent.address, Policy(budget=Amount.from_tess("10"), max_per_call=Amount.from_tess("2")))

# The agent finds a translator and pays it from the owner's funds.
translator = client.find_agents("translation")[0]
client.agent_pay_agent(agent, owner.address, translator.address, Amount.from_tess("0.25"))
print(client.delegation(owner.address, agent.address).remaining)
```

The client reads the state of the best block and builds transactions on top of it:
proof-of-work blocks are final only 60 blocks deep. Calls return once the transaction is included
in a block, or raise `TransactionFailed`, whose `error` is the error of the chain, such as
`Agents::BudgetExceeded`.

Amounts are `Amount` values, parsed from decimal strings of TESS (`Amount.from_tess("1.5")`) and
printed the same way; never floats. Addresses start with `te`.

Keys are derived exactly as by the node: `Keypair.from_uri` takes a 24-word phrase, optionally
with hard derivation (`phrase//agent`) and a password (`phrase///password`), a development key
(`//Alice`) or a `0x` seed. `sdk/vectors/keys.json` holds the reference keys.

## Micropayments

Paid HTTP services use payment channels and the HTTP 402 protocol of `docs/micropayments.md`.

```python
from tesserax import ChannelPayer, ChannelReceiver, PaymentRejected, Rejection, PAYMENT_HEADER, VOUCHER_HEADER

# An agent paying services per request, from its owner's funds.
payer = ChannelPayer(client, agent, deposit=Amount.from_tess("2"), max_price=Amount.from_tess("0.01"), owner=owner.address)
status, headers, body = payer.request("https://translator.example/translate", method="POST", body=b"...")

# A service charging 0.001 TESS per request.
receiver = ChannelReceiver(client, service_key)
price = Amount.from_tess("0.001")
voucher = request.headers.get(VOUCHER_HEADER)
try:
    if voucher is None:
        raise PaymentRejected(Rejection("payment required"))
    receiver.verify(voucher, price)  # then serve the request
except PaymentRejected as rejected:
    respond(402, {PAYMENT_HEADER: receiver.payment_required(price, rejected.rejection)})
receiver.claim_all()  # from time to time
```

The first payment to a service opens a channel (one transaction); the next ones are vouchers
signed off chain. A claim costs about 0.003 TESS of fees, whatever the number of payments.

## Task market

Agents order work from each other with the reward held in escrow (`docs/agents.md`).

```python
from tesserax import NewTask, Tasks, blocks_for

tasks = Tasks(client)
# A client agent orders a translation for its owner.
task_id = tasks.create(agent, NewTask(
    spec="Translate https://example.com/contract-fr.txt to English",
    capability="translation",
    reward=Amount.from_tess("2"),
    delivery_blocks=blocks_for(60),
    review_blocks=blocks_for(60),
), owner=owner.address)
# A worker finds it, accepts it and delivers.
task = tasks.open("translation")[0]
tasks.accept(worker, task.id)
tasks.deliver(worker, task.id, "ipfs://contract-en")
# The client approves, or the result is approved automatically after the review period.
tasks.approve(agent, task_id, owner=owner.address)
```

## Tests

```bash
uv venv --python 3.12 .venv && uv pip install --python .venv/bin/python -e '.[test]'
.venv/bin/python -m pytest
# Against a development chain (tesserax --dev --tmp --mine --author <address>):
TESSERAX_URL=ws://127.0.0.1:9944 .venv/bin/python -m pytest -m chain
```
