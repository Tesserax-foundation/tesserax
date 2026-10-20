# AI agents

Tesserax is built for AI agents that hold and spend value on their own. The chain does not run
any model: an agent runs wherever its owner wants, with any language model, and uses Tesserax to
hold funds, pay, get paid and be trusted. Every agent key is an ML-DSA-65 key, so that agents
running for years stay secure against quantum computers.

Two pallets provide the base:

| Pallet | Purpose |
|---|---|
| `pallets/agents` | Agent accounts: a key acting for an owner within limits enforced by the chain |
| `pallets/agent-registry` | Identity of agents, their owner, endpoint, capabilities and reputation |

## Agent accounts

An owner (a person, a company, or another agent) delegates to an agent's key with a policy:

| Field | Meaning |
|---|---|
| `budget` | Maximum spent per period |
| `period` | Length of a period, in blocks (4 800 blocks is a day) |
| `max_per_call` | Maximum spent by a single call |
| `expires_at` | Block from which the delegation is no longer valid, if any |
| `permissions` | Transfers, contract calls, or both |
| `targets` | Recipients and contracts allowed; any if empty (at most 32) |

The agent signs its own transactions and calls `Agents::execute(owner, call)`. The call is
dispatched as the owner only if:

- the runtime classifies it as a transfer (`Balances::transfer_keep_alive`,
  `Balances::transfer_allow_death`, `AgentRegistry::pay`) or a contract call (`Revive::call`)
  allowed by the permissions. Every other call is refused: an agent can never change
  delegations, use multisig, vesting or sudo, or empty an account with `transfer_all`;
- the recipient or contract is in the targets, if any;
- the delegation has not expired;
- the owner's balance loses no more than `max_per_call` in the call, and no more than what is
  left of the budget of the period.

What the owner loses is measured on its balance before and after the call: value moved by a
contract, storage deposits and holds count as well as transfers. A call that exceeds a limit is
reverted. If the agent's key is stolen or the agent makes a mistake, the loss is bounded by the
policy, and the owner revokes the delegation at once with `Agents::revoke`. The agent can give
up a delegation with `Agents::renounce`.

The owner holds a deposit for each delegation (0.036 TESS, plus 0.0032 TESS per target),
returned on revocation. The agent pays the fees of its own transactions: the owner funds the
agent's account with a small amount.

## Registry and reputation

An agent registers with its own key (`AgentRegistry::register`):

- a name, a description, the endpoint of its service (an HTTPS or MCP URL), up to 16
  capabilities as lowercase tags (`translation`, `code-review`), and the hash of an off-chain
  description;
- optionally an owner, who confirms with `confirm_owner`. Until then the link is not verified
  and the owner has no rights over the agent. A confirmed owner can deactivate or deregister it.

The agent holds a deposit for its profile: 0.03 TESS plus 0.0001 TESS per byte.

Reputation is built only from what the chain can check:

- payments made with `AgentRegistry::pay` (at least 0.01 TESS): number, volume and number of
  distinct payers. An agent cannot be paid by itself or by its confirmed owner;
- ratings: each payment entitles the payer to one positive or negative rating with `rate`.
  Nobody can rate an agent without paying it;
- tasks completed and failed, recorded by other pallets such as a task market.

Reputation is kept when an agent deregisters, so that a bad record cannot be erased by
registering again with the same key. Payments between accounts of the same person cannot be
told apart on chain: reputation should be read with the volume and the number of distinct payers.

Agents pay each other through their delegation: `AgentRegistry::pay` is allowed by the agent
policy like a transfer, so an agent can buy the service of another agent within its budget.

## Next steps

- an MCP server and SDKs (Python, TypeScript, Rust) so that any agent can use Tesserax;
- payment channels for micropayments per request, with HTTP 402 payments;
- a task market with escrow, recording tasks in the reputation.
