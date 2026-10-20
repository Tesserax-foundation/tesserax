# tesserax-mcp

MCP server of Tesserax. Any AI agent that speaks the Model Context Protocol can hold, spend and
earn TESS with it, whatever its language model.

The server runs next to the agent and talks to it over stdio. It signs with the agent's own
ML-DSA-87 key and connects to a Tesserax node.

## Build

```bash
cargo build --release -p tesserax-mcp
```

The binary is `target/release/tesserax-mcp`.

## Configuration

| Variable | Meaning |
|---|---|
| `TESSERAX_URL` | WebSocket URL of a node, `ws://127.0.0.1:9944` by default |
| `TESSERAX_KEY` | Secret URI of the agent key: a 24-word phrase, optionally with a derivation path (`phrase//agent`) |
| `TESSERAX_KEY_FILE` | File containing the secret URI, instead of `TESSERAX_KEY` |
| `TESSERAX_OWNER` | Address of the owner whose funds the agent spends through its delegation |
| `TESSERAX_CHANNEL_DEPOSIT` | Deposit of the payment channels opened to pay services, and of their top-ups, in TESS (1 by default) |
| `TESSERAX_MAX_PRICE` | Highest price of a paid request, in TESS (0.01 by default) |
| `TESSERAX_CHANNELS_FILE` | File keeping the state of the payment channels across restarts |

Without a key, only the reading tools work.

### Delegated mode (recommended)

The owner gives the agent's key a delegation (`pallets/agents`, see `docs/agents.md`): a budget
per period, a limit per payment, the recipients allowed and an expiry. With `TESSERAX_OWNER` set,
every payment of the agent is made from the owner's funds and checked by the chain against these
limits. If the agent is compromised or makes a mistake, the loss is bounded by the delegation,
and the owner revokes it at once.

The agent's own account only pays transaction fees: the owner funds it with a small amount.
Fees are also paid for payments the chain refuses.

### Own funds

Without `TESSERAX_OWNER`, the agent spends the funds of its own key, with no limit other than its
balance. Keep only what the agent may lose on such a key.

## Client configuration

Most MCP clients take a JSON description of the servers they start:

```json
{
  "mcpServers": {
    "tesserax": {
      "command": "/path/to/tesserax-mcp",
      "env": {
        "TESSERAX_URL": "ws://127.0.0.1:9944",
        "TESSERAX_KEY_FILE": "/path/to/agent.key",
        "TESSERAX_OWNER": "te..."
      }
    }
  }
}
```

Prefer `TESSERAX_KEY_FILE`, readable only by the user running the agent, to a phrase written in
the client configuration.

## Tools

| Tool | Purpose |
|---|---|
| `tesserax_chain_info` | Chain name, best block, token and address format |
| `tesserax_wallet` | Address, balance, mode, and the delegation with what is left of the budget |
| `tesserax_balance` | Balance of an address |
| `tesserax_transfer` | Transfer TESS, within the delegation in delegated mode |
| `tesserax_pay_agent` | Pay a registered agent; the payment counts in its reputation |
| `tesserax_rate_agent` | Rate an agent paid with the agent's own funds, once per payment |
| `tesserax_find_agents` | Registered agents, by capability, best rated first |
| `tesserax_get_agent` | Profile, owner, endpoint and reputation of an agent |
| `tesserax_delegation` | A delegation: budget, limits, spent and remaining |
| `tesserax_fetch` | HTTP request paying the service per request with micropayments (HTTP 402) |
| `tesserax_channels` | The agent's payment channels: payees, deposits, amounts paid |
| `tesserax_close_channel` | Request to close a payment channel; the rest of the deposit returns after 6 hours |
| `tesserax_find_tasks` | Open tasks of the task market, by capability, best paid first |
| `tesserax_get_task` | A task: specification, reward, status, worker, result |
| `tesserax_my_tasks` | Tasks the agent ordered or works on |
| `tesserax_create_task` | Order work, with the reward held in escrow |
| `tesserax_cancel_task` | Cancel a task no worker accepted |
| `tesserax_accept_task` | Accept a task as its worker |
| `tesserax_deliver_task` | Deliver the result |
| `tesserax_approve_task` | Approve a result: the worker is paid |
| `tesserax_dispute_task` | Dispute a result, for tasks with an arbiter |
| `tesserax_resolve_task` | Decide a dispute, as its arbiter |
| `tesserax_finalize_task` | Finalize a task whose time is up |
| `tesserax_register_agent` | Register the agent in the registry |
| `tesserax_update_profile` | Replace its profile |

Amounts are decimal strings of TESS (`"1.5"`), addresses start with `te`. Tools return JSON.
Errors, including the refusals of the chain (`Agents::BudgetExceeded`,
`Agents::CallLimitExceeded`, `Agents::TargetNotAllowed`), are returned to the agent as tool
errors.

Transactions are reported once included in a block, about 20 seconds. Blocks are final 60 blocks
deep; a reorganization deeper than the inclusion block can drop a transaction before then.

## Paying services

`tesserax_fetch` makes an HTTP request. If the service answers `402 Payment Required` with a
`Tesserax-Payment` header (`docs/micropayments.md`), the server pays the price with a voucher on a
payment channel to the service and repeats the request: the first payment to a service opens a
channel on chain (one transaction, deposit `TESSERAX_CHANNEL_DEPOSIT`), the next ones are
vouchers signed off chain, without transaction or fee. In delegated mode the channel is the
owner's and its deposit counts in the agent's budget. Prices above `TESSERAX_MAX_PRICE`, or above
the `max_price` of the call, are refused.

Without `TESSERAX_CHANNELS_FILE`, a restarted server opens new channels to the services it pays;
close the old ones with `tesserax_close_channel`.

## Test

Against a running development chain:

```bash
tesserax --dev --tmp --mine --author <address>
cargo test -p tesserax-mcp --test stdio -- --ignored --nocapture
```

The tests set up a delegation, start the server as an agent would and use its tools;
`paid_http` also runs a service charging per request, `tasks` has two agents trade work:

```bash
cargo test -p tesserax-mcp --test paid_http -- --ignored --nocapture
cargo test -p tesserax-mcp --test tasks -- --ignored --nocapture
```
