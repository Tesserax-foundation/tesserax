# Bug bounty

Tesserax pays for vulnerabilities found before and after the launch of the mainnet. The program
starts with **25,000 TESS**, paid by the foundation from its liquid allocation, in TESS of the
mainnet.

## Rewards

| Severity | Examples | Reward |
|---|---|---|
| Critical | minting or stealing TESS; spending a shielded note twice or creating value in the shielded pool; forging an ML-DSA signature the chain accepts; passing a referendum without its votes, a call without its track, or acting as the technical committee; halting or splitting the network with a few blocks or transactions; taking the funds or the address of another account in contracts | 5,000 to 10,000 TESS |
| High | linking the sender, recipient or amount of private transfers without the keys; stealing from an agent beyond its delegation, or from a payment channel or task escrow; reading keys out of the wallet, its extension or its desktop application; crashing every node from the network or the public RPC; downgrading the post-quantum TLS of the peer-to-peer network or of the public RPC to a classical key exchange | 1,500 to 4,000 TESS |
| Medium | crashing one node remotely; making blocks or transactions far cheaper than their weight (spam); draining the faucet or the pool beyond their limits; script injection in the wallet or the explorer; a vote counting TESS still vesting | 300 to 1,000 TESS |
| Low | a flaw with a limited impact, or only under unlikely conditions, worth fixing | 50 to 200 TESS |

The reward depends on the impact demonstrated and on the quality of the report: a working proof
of concept against a development chain or the testnet, and a fix, are worth the top of the range.
The first valid report of an issue is rewarded; later reports of the same issue are not.

When the 25,000 TESS are spent, the program pauses until it is funded again; this page says so.

## Scope

The public repositories and the services of the testnet:

- [`tesserax`](https://github.com/Wayleyy/tesserax): the node, the runtime and its pallets
  (consensus and difficulty, emission, agents and their registry, payment channels, tasks,
  shielded pool, governance), the post-quantum cryptography and networking (`primitives/`,
  `vendor/`), the forked smart contract pallet, the SDKs, the command line wallet, the RPC gateway,
  the faucet, the pool and the explorer indexer;
- [`tesserax-miner`](https://github.com/Wayleyy/tesserax-miner): the `rx/tess` miner, for issues
  that affect the chain, such as a hash the network accepts but should not;
- the testnet and its services: `wss://rpc.tesserax.app:21207`, https://wallet.tesserax.app,
  https://explorer.tesserax.app, https://faucet.tesserax.app and https://pool.tesserax.app with
  `stratum.tesserax.app:25202`.

Out of scope:

- denial of service by volume against the hosted services (flooding the RPC, the faucet, the pool
  or the web pages): test it on your own node;
- the hosting providers themselves, social engineering, physical attacks, and the keys of the
  team;
- vulnerabilities of third-party dependencies that do not affect Tesserax;
- issues already known: the list below, and those reported first.

## Known issues

These are documented and need no report, unless you find a way to make them worse than described:

- Transaction weights were measured on a development machine, not on the reference hardware.
- The ML-DSA implementations (`ml-dsa` in the runtime, AWS-LC in the networking) are not audited
  for Tesserax.
- Addresses in contracts are 160 bits: taking the address of a given account costs a second
  preimage of 160 bits ([contracts.md](contracts.md#addresses)).
- A root referendum can change anything, the rules of governance included
  ([governance.md](governance.md#limits)); TESS still vesting can fund deposits, which give no vote.
- The runtime of the genesis of the testnet contains the paths of the machine that built it.
- The public RPC listens on port 21207, and the pool behind a TCP proxy that hides the addresses
  of the rigs, which disables its limits per IP address.

## Rules

- Test against your own development chain (`tesserax --dev`) whenever you can, and against the
  testnet only as far as needed to show the issue. Never against the mainnet or the funds of
  others: move only TESS you hold, and give back what a test moved.
- Report privately, with the steps to reproduce, the impact, and if possible a proof of concept.
- Give us 90 days to fix the issue before disclosing it, or less once the fix is released. We
  publish the issues fixed and, if you wish, your name.
- One issue per report. Chained issues are one report, rewarded for their combined impact.
- Acting in good faith within these rules, you will not face any claim from the foundation for
  your research.

## Report

Report on GitHub, privately: **Security → Report a vulnerability** on the repository
[`tesserax`](https://github.com/Wayleyy/tesserax/security), or on `tesserax-miner` for the miner.
Do not open a public issue.

We answer within 3 days, assess the severity within 10, and keep you informed until the fix.
Give a Tesserax address (`te...`) for the reward: it is paid in TESS of the mainnet, within 30
days of the fix or of the launch of the mainnet, whichever comes later, by the multisig of the
foundation.

## Rewarded

| Date | Issue | Severity | Reward |
|---|---|---|---|
| | No report yet | | |

Spent: 0 of 25,000 TESS.
