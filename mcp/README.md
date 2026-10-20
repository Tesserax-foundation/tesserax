# tesserax-mcp

MCP server of Tesserax. Any AI agent that speaks the Model Context Protocol can hold, spend and
earn TESS with it, whatever its language model.

The server runs next to the agent and talks to it over stdio. It signs with the agent's own
ML-DSA-65 key and connects to a Tesserax node.

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
| `tesserax_register_agent` | Register the agent in the registry |
| `tesserax_update_profile` | Replace its profile |

Amounts are decimal strings of TESS (`"1.5"`), addresses start with `te`. Tools return JSON.
Errors, including the refusals of the chain (`Agents::BudgetExceeded`,
`Agents::CallLimitExceeded`, `Agents::TargetNotAllowed`), are returned to the agent as tool
errors.

Transactions are reported once included in a block, about 20 seconds. Blocks are final 60 blocks
deep; a reorganization deeper than the inclusion block can drop a transaction before then.

## Test

Against a running development chain:

```bash
tesserax --dev --tmp --mine --author <address>
cargo test -p tesserax-mcp --test stdio -- --ignored --nocapture
```

The test sets up a delegation, starts the server as an agent would and uses its tools.
