# Governance

Tesserax has no sudo key. Changes to the chain, a runtime upgrade first of all, are decided by
referenda, which holders of TESS vote on. Nobody, neither the team nor the foundation, can change
the chain alone.

## Referenda

Anyone proposes a call, such as `system.set_code` with a new runtime, on the track allowed to
dispatch it. A referendum then goes through:

1. **Preparation**: it waits for its decision deposit and the end of its preparation period, so
   that voters see it before the vote starts.
2. **Decision**: holders vote for (aye) or against (nay). It passes once it has both the approval
   (ayes over all votes) and the support (TESS voting aye over the TESS that may vote) its track
   requires; both requirements decrease over the decision period.
3. **Confirmation**: it must keep passing for the confirmation period, so that a last-minute vote
   cannot pass it.
4. **Enactment**: its call is dispatched after the enactment delay, with the origin of its track,
   by the chain itself.

A referendum that does not pass by the end of its decision period is rejected.

### Tracks

| Track | What it may do | Decision deposit | Preparation | Decision | Confirmation | Enactment | Approval | Support |
|---|---|---|---|---|---|---|---|---|
| Root | anything, a runtime upgrade included | 1,000 TESS | 2 hours | 14 days | 1 day | 1 day | 100% to 50% | 20% to 1% |
| Urgent | a call the technical committee whitelisted, dispatched as root | 500 TESS | 30 minutes | 2 days | 10 minutes | 10 minutes | 100% to 50% | 5% to 0.5% |
| Cancellation | cancel a referendum, refunding its deposits, or kill it, slashing them | 100 TESS | 1 hour | 1 day | 3 hours | 10 minutes | 100% to 50% | 10% to 0.5% |

Approval and support decrease linearly over the decision period, from the first value to the
second. Submitting a referendum holds 10 TESS more, and storing its call holds 2 TESS plus 0.0001
TESS per byte, about 100 TESS for a runtime: all of it is returned once the referendum ends,
unless a cancellation referendum kills it. Slashed deposits go to the miner of the block, like
fees: nothing is burned.

## Votes

A vote locks TESS, it never spends them; the TESS stay in the account. Conviction multiplies a
vote against a longer lock, counted after the referendum ends, if it ends the way of the vote:

| Conviction | Votes | Lock after the referendum |
|---|---|---|
| 0 | 0.1x | none |
| 1 | 1x | 1 week |
| 2 | 2x | 2 weeks |
| 3 | 3x | 4 weeks |
| 4 | 4x | 8 weeks |
| 5 | 5x | 16 weeks |
| 6 | 6x | 32 weeks |

Holders may delegate their votes on a track to another account, without giving it their TESS.

**TESS still vesting do not vote.** Votes and delegations are capped by the balance of an account
without its TESS still vesting, and support is measured against the TESS that may vote: the total
issuance without what the genesis allocations still vest. The allocations of the team and of the
foundation weigh nothing while they vest, and do not raise the support referenda need. They may
still fund deposits, which give no vote.

AI agents never vote with the TESS of their owners: delegations to agents cannot call governance.

## Technical committee

The technical committee is the three signatories of the foundation: the two members of the team
and the backup key. Two of them may whitelist a call, which then needs a referendum of the urgent
track instead of root: two days instead of fourteen, for fixes that cannot wait. A call they have
not whitelisted fails on the urgent track, whatever its votes.

The committee can do nothing else: it dispatches no call itself, cancels no referendum, and its
members never change, no call can change them.

## Using it

With the command line wallet:

```bash
tesserax-wallet governance list
tesserax-wallet governance show 4
tesserax-wallet governance propose-upgrade alice tesserax_runtime.compact.compressed.wasm
tesserax-wallet governance propose alice 0x<encoded call> --track root
tesserax-wallet governance vote alice 4 aye 1000 --conviction 3
tesserax-wallet governance unvote alice 4
tesserax-wallet governance unlock alice
tesserax-wallet governance delegate alice te... 1000 --conviction 1 --track root
tesserax-wallet governance cancel alice 4 [--kill]

# The technical committee: the first member approves, the second one whitelists.
tesserax-wallet governance whitelist member1 0x<encoded call or its hash>
tesserax-wallet governance whitelist member2 0x<encoded call or its hash>
tesserax-wallet governance propose alice 0x<encoded call> --track urgent
```

Proposed calls are checked: a call that does not decode as a call of the runtime is refused. The
web wallet, its extension and its desktop application have a Governance page: the referenda
ongoing with their approval and support against what they need, votes with conviction, removing
votes and unlocking. The Rust SDK has the same operations (`tesserax_sdk::governance`).

## Runtime upgrades

1. Build the runtime reproducibly (`scripts/build-runtime.sh`), with a higher `spec_version`, and
   publish its hash with the sources, so that voters can rebuild it and compare.
2. Propose it: `tesserax-wallet governance propose-upgrade`.
3. Once approved, confirmed and enacted, from two days later with a strong support from the
   start to sixteen days, the chain runs it from the next block. Nodes need no update: they run
   the runtime of the chain.

The runtime of a development chain can shorten every period to minutes, with the `fast-runtime`
feature of the node, to try referenda locally; it is never the runtime of a public network.

## Limits

A root referendum can change anything in the runtime, the rules of governance and the supply
included: only its periods, its thresholds and the vote of holders protect the chain. The
technical committee shortens the time a fix takes, never the vote it needs.
