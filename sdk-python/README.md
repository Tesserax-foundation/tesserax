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
payer = ChannelPayer(
    client, agent, deposit=Amount.from_tess("2"), max_price=Amount.from_tess("0.01"), owner=owner.address,
    state_file="agent-channels.json",  # resumes its channels after a restart
)
status, headers, body = payer.request("https://translator.example/translate", method="POST", body=b"...")

# A service charging 0.001 TESS per request.
receiver = ChannelReceiver(client, service_key, state_file="service-channels.json")  # keeps unclaimed vouchers
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

## Smart contracts

Solidity compiled to PolkaVM with `resolc`, deployed and called with ML-DSA signed transactions;
functions called by name, arguments and results encoded with the Solidity ABI, events and revert
reasons decoded (`docs/contracts.md`). Reads are dry runs, without a transaction or a fee.

```python
from tesserax import to_h160

token = client.deploy_contract(alice, code, abi, args=["Tesserax Coin", 1_000_000])
token.write(alice, "transfer", [to_h160(bob), 250])
balance = token.read("balanceOf", [to_h160(bob)])
for event in token.watch_events():
    print(event.name, event.args)
```

## Private transactions

Every account has a shielded account, restored by its recovery phrase. Proofs are generated
locally by the shielded wallet compiled to WebAssembly, the same module on every platform, run
by wasmtime: install the extra, `pip install "tesserax[shielded]"` (`docs/privacy.md`).

```python
from tesserax import Amount, ShieldedAccount

shielded = ShieldedAccount.create(alice)
shielded.sync(client)                                  # reads the chain since its last block
shielded.shield(client, alice, Amount.from_tess("10"))  # public balance -> private
shielded.send(client, "zte...", Amount.from_tess("4"))  # private, to a shielded address
shielded.unshield(client, "te...", Amount.from_tess("3"))  # private -> a public address
state = shielded.state()                               # ShieldedAccount.restore(alice, state)
```

## Swaps

TESS against ETH, USDC and USDT on Ethereum, Base and Arbitrum, with atomic swaps (`docs/swaps.md`
of the chain). `client.swaps` is the Tesserax side; `EvmSwaps` the EVM side, with web3.py
(`pip install "tesserax[swaps]"`):

```python
from tesserax import Amount, EvmSwaps, evm_networks, new_secret, payment_due

offer = client.swaps.offers()[0]
evm = EvmSwaps(next(n for n in evm_networks(testnet=True) if n.chain == offer.payment.chain),
               private_key=open("taker.key").read().strip())
secret, hash_ = new_secret()                    # keep the secret until the claim
amount = Amount.from_tess("50")
lock = evm.lock_payment(offer.payment.asset, "0x" + offer.payout.hex(),
                        payment_due(amount.planck, offer.price), hash_,
                        evm.head()[1] + 86_400, offer.id, "te...")   # receives the TESS, needs none
# Once the maker filled it:
trade = client.swaps.trade_of_lock(offer.payment.chain, lock)
client.swaps.claim(trade, secret)               # unsigned, without fee
```

Makers offer with `client.swaps.create_offer`, find the payments locked for them with
`EvmSwaps.locks`, fill them with `client.swaps.fill`, read the secrets claims reveal with
`client.swaps.events`, and collect with `EvmSwaps.claim`.

In BTC, `tesserax.bitcoin` (embit) builds the script and the payment, with the offer announced to
the maker: `lock_transaction` signs it with a Bitcoin key, `find_lock` is how a maker finds it,
`claim_transaction` collects it with the secret, `refund_transaction` takes it back after 24 hours,
with the key paying or with `refund_key`, derived from the seed of the account as the web wallet
derives it. `Esplora` reads and broadcasts through the API of mempool.space.

## Tests

```bash
uv venv --python 3.12 .venv && uv pip install --python .venv/bin/python -e '.[test]'
.venv/bin/python -m pytest
# Against a development chain (tesserax --dev --tmp --mine --author <address>):
TESSERAX_URL=ws://127.0.0.1:9944 .venv/bin/python -m pytest -m chain
# The swap test also needs a local EVM chain with the swap contract (swaps/evm/deploy.sh):
TESSERAX_URL=ws://127.0.0.1:9944 EVM_URL=http://127.0.0.1:8545 .venv/bin/python -m pytest tests/test_swaps.py
```
