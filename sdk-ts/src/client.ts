/** Client of a Tesserax node. */

import { createClient, Enum, type PolkadotClient, type TxEvent } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";
import { lastValueFrom, filter, map, take } from "rxjs";
import { Amount, DECIMALS } from "./amount.js";
import { fromHex, Keypair, formatAddress, parseAddress, SS58_PREFIX, toHex } from "./keys.js";
import { Contract, Contracts, type ContractTxOutcome, type DeployOptions } from "./contracts.js";
import { chainError, TransactionFailed } from "./errors.js";
import { mlDsaTxCreator } from "./signer.js";
import { Swaps } from "./swaps.js";

export { chainError, TransactionFailed };

/** Blocks a transaction stays valid, counted from the best block: about 19 minutes. */
export const MORTALITY = 64;

export interface ChainInfo {
  chain: string;
  bestBlock: number;
  specVersion: number;
  token: "TESS";
  decimals: number;
  ss58Prefix: number;
}

export interface Balance {
  free: Amount;
  reserved: Amount;
  frozen: Amount;
  transferable: Amount;
}

export interface TxOutcome {
  blockHash: string;
  blockNumber: number;
  txHash: string;
  /** Events emitted by the transaction. */
  events: { type: string; value: { type: string; value: unknown } }[];
}

/** Limits of a delegation from an owner to an agent. */
export interface Policy {
  budget: Amount;
  maxPerCall: Amount;
  /** Length of a period, in blocks of 18 seconds; 4 800 (a day) by default. */
  periodBlocks?: number;
  expiresAt?: number;
  transfers?: boolean;
  contracts?: boolean;
  /** Recipients and contracts allowed, as addresses; any if empty. */
  targets?: string[];
}

export interface Delegation {
  owner: string;
  agent: string;
  policy: Required<Omit<Policy, "expiresAt">> & { expiresAt?: number };
  spent: Amount;
  periodStart: number;
  remaining: Amount;
  expired: boolean;
}

export interface Profile {
  name: string;
  description?: string;
  /** Where the service of the agent is reached, such as an HTTPS or MCP URL. */
  endpoint?: string;
  /** Lowercase tags such as "translation". */
  capabilities?: string[];
  /** Hexadecimal 32-byte hash of an off-chain description. */
  metadata?: string;
}

export interface Reputation {
  payments: bigint;
  volume: Amount;
  payers: number;
  positive: number;
  negative: number;
  tasksCompleted: number;
  tasksFailed: number;
}

export interface AgentRecord {
  address: string;
  owner?: string;
  ownerConfirmed: boolean;
  active: boolean;
  registeredAt: number;
  profile: Required<Omit<Profile, "metadata">> & { metadata?: string };
  reputation: Reputation;
}

const utf8 = new TextEncoder();
const text = (value: Uint8Array) => new TextDecoder().decode(value);

const bounded = (value: string, limit: number, name: string) => {
  const bytes = utf8.encode(value);
  if (bytes.length > limit) throw new RangeError(`${name} is longer than ${limit} bytes`);
  return bytes;
};

// Accounts are given to the codecs as addresses with the Tesserax prefix.
const address = (value: string) => formatAddress(parseAddress(value));

/**
 * A connection to a Tesserax node.
 *
 * The client reads the state of the best block and builds transactions on top of it:
 * proof-of-work blocks are final only 60 blocks deep. Transactions resolve once included in a
 * block, or reject with `TransactionFailed`.
 */
export class TesseraxClient {
  readonly client: PolkadotClient;
  // The SDK is not generated from metadata: calls and storage are reached by name.
  readonly api: any;
  /** Smart contracts: dry runs, deployments, calls and events. */
  readonly contracts: Contracts;
  /** Atomic swaps of TESS: offers, trades and their calls. */
  readonly swaps: Swaps;

  constructor(url = "ws://127.0.0.1:9944") {
    this.client = createClient(getWsProvider(url));
    this.api = this.client.getUnsafeApi();
    this.contracts = new Contracts(this);
    this.swaps = new Swaps(this);
  }

  destroy(): void {
    this.client.destroy();
  }

  async chainInfo(): Promise<ChainInfo> {
    const [spec, best] = await Promise.all([
      this.client.getChainSpecData(),
      this.client.getBestBlocks(),
    ]);
    const version = await this.api.constants.System.Version();
    return {
      chain: spec.name,
      bestBlock: best[0].number,
      specVersion: version.spec_version,
      token: "TESS",
      decimals: DECIMALS,
      ss58Prefix: SS58_PREFIX,
    };
  }

  /** Number of the best block. */
  async bestBlockNumber(): Promise<number> {
    return (await this.client.getBestBlocks())[0].number;
  }

  /** Genesis hash of the chain, which vouchers sign. */
  async genesisHash(): Promise<Uint8Array> {
    return fromHex((await this.client.getChainSpecData()).genesisHash);
  }

  async balance(of: string): Promise<Balance> {
    const { data } = (await this.api.query.System.Account.getValue(address(of), { at: "best" })) as {
      data: { free: bigint; reserved: bigint; frozen: bigint };
    };
    return {
      free: new Amount(data.free),
      reserved: new Amount(data.reserved),
      frozen: new Amount(data.frozen),
      transferable: new Amount(data.free > data.frozen ? data.free - data.frozen : 0n),
    };
  }

  /** Sign `tx` with `signer`, submit it and wait until a block includes it successfully. */
  async submit(tx: { createSubmitAndWatch: Function }, signer: Keypair): Promise<TxOutcome> {
    const events = tx.createSubmitAndWatch(mlDsaTxCreator(signer), {
      mortality: { mortal: true, period: MORTALITY },
    });
    let included: Extract<TxEvent, { type: "inBestBlock" }>;
    try {
      included = await lastValueFrom(
        events.pipe(
          filter((e: TxEvent) => e.type === "inBestBlock" || e.type === "finalized"),
          take(1),
          map((e: TxEvent) => e as Extract<TxEvent, { type: "inBestBlock" }>),
        ),
      );
    } catch (error) {
      throw new TransactionFailed(`transaction refused: ${(error as Error).message}`);
    }
    if (!included.ok) {
      const error = chainError(included.dispatchError);
      throw new TransactionFailed(`transaction failed: ${error}`, error);
    }
    return {
      blockHash: included.block.hash,
      blockNumber: included.block.number,
      txHash: included.txHash,
      events: included.events as unknown as TxOutcome["events"],
    };
  }

  // Smart contracts.

  /** Deploy a contract compiled to PolkaVM, signed by `signer`, and return it once a block
   * includes the deployment, with the events of its constructor. */
  async deployContract(signer: Keypair, options: DeployOptions): Promise<Contract & { deployment: ContractTxOutcome }> {
    const { contract, outcome } = await this.contracts.deploy(signer, options);
    return Object.assign(contract, { deployment: outcome });
  }

  /** The contract at `address` (`0x` and 40 digits), called through `abi`. */
  contract(address: string, abi: Contract["abi"]): Contract {
    return new Contract(this, address as `0x${string}`, abi);
  }

  /** Transfer `amount` to `to`, keeping the signer's account alive. */
  transfer(signer: Keypair, to: string, amount: Amount): Promise<TxOutcome> {
    const tx = this.api.tx.Balances.transfer_keep_alive({ dest: Enum("Id", address(to)), value: amount.planck });
    return this.submit(tx, signer);
  }

  // Delegations.

  /** Delegate to `agent` within `policy`, signed by the owner. */
  authorizeAgent(owner: Keypair, agent: string, policy: Policy): Promise<TxOutcome> {
    const tx = this.api.tx.Agents.authorize({ agent: address(agent), policy: encodePolicy(policy) });
    return this.submit(tx, owner);
  }

  revokeAgent(owner: Keypair, agent: string): Promise<TxOutcome> {
    return this.submit(this.api.tx.Agents.revoke({ agent: address(agent) }), owner);
  }

  renounceDelegation(agent: Keypair, owner: string): Promise<TxOutcome> {
    return this.submit(this.api.tx.Agents.renounce({ owner: address(owner) }), agent);
  }

  /** The delegation of `owner` to `agent`, if any, with what is left of its budget. */
  async delegation(owner: string, agent: string): Promise<Delegation | undefined> {
    const [d, best] = await Promise.all([
      this.api.query.Agents.Delegations.getValue(address(owner), address(agent), { at: "best" }),
      this.client.getBestBlocks(),
    ]);
    if (!d) return undefined;
    const now = best[0].number;
    const p = d.policy;
    const policy = {
      budget: new Amount(p.budget),
      maxPerCall: new Amount(p.max_per_call),
      periodBlocks: p.period as number,
      expiresAt: p.expires_at as number | undefined,
      transfers: p.permissions.transfers as boolean,
      contracts: p.permissions.contracts as boolean,
      targets: (p.targets as string[]).map(address),
    };
    const expired = policy.expiresAt !== undefined && now >= policy.expiresAt;
    let remaining = 0n;
    if (!expired) {
      remaining =
        now >= d.period_start + policy.periodBlocks
          ? policy.budget.planck
          : policy.budget.planck > d.spent
            ? policy.budget.planck - d.spent
            : 0n;
    }
    return {
      owner: address(owner),
      agent: address(agent),
      policy,
      spent: new Amount(d.spent),
      periodStart: d.period_start,
      remaining: new Amount(remaining),
      expired,
    };
  }

  /** Make a call for `owner`, signed by the agent, within its delegation. */
  agentExecute(agent: Keypair, owner: string, call: { decodedCall: unknown }): Promise<TxOutcome> {
    const tx = this.api.tx.Agents.execute({ owner: address(owner), call: call.decodedCall });
    return this.submit(tx, agent);
  }

  /** Transfer `amount` from `owner` to `to`, signed by the agent, within its delegation. */
  agentTransfer(agent: Keypair, owner: string, to: string, amount: Amount): Promise<TxOutcome> {
    const call = this.api.tx.Balances.transfer_keep_alive({ dest: Enum("Id", address(to)), value: amount.planck });
    return this.agentExecute(agent, owner, call);
  }

  /** Pay the registered agent `payee` from `owner`'s funds, within the delegation. */
  agentPayAgent(agent: Keypair, owner: string, payee: string, amount: Amount): Promise<TxOutcome> {
    const call = this.api.tx.AgentRegistry.pay({ agent: address(payee), amount: amount.planck });
    return this.agentExecute(agent, owner, call);
  }

  // Registry.

  /** Register `signer` as an agent, optionally naming its owner, who must confirm. */
  registerAgent(signer: Keypair, profile: Profile, owner?: string): Promise<TxOutcome> {
    const tx = this.api.tx.AgentRegistry.register({
      profile: encodeProfile(profile),
      owner: owner ? address(owner) : undefined,
    });
    return this.submit(tx, signer);
  }

  updateProfile(signer: Keypair, profile: Profile): Promise<TxOutcome> {
    return this.submit(this.api.tx.AgentRegistry.update_profile({ profile: encodeProfile(profile) }), signer);
  }

  confirmOwner(owner: Keypair, agent: string): Promise<TxOutcome> {
    return this.submit(this.api.tx.AgentRegistry.confirm_owner({ agent: address(agent) }), owner);
  }

  setAgentActive(signer: Keypair, agent: string, active: boolean): Promise<TxOutcome> {
    return this.submit(this.api.tx.AgentRegistry.set_active({ agent: address(agent), active }), signer);
  }

  /** Pay the registered agent `agent`; the payment counts in its reputation. */
  payAgent(payer: Keypair, agent: string, amount: Amount): Promise<TxOutcome> {
    return this.submit(this.api.tx.AgentRegistry.pay({ agent: address(agent), amount: amount.planck }), payer);
  }

  rateAgent(payer: Keypair, agent: string, positive: boolean): Promise<TxOutcome> {
    return this.submit(this.api.tx.AgentRegistry.rate({ agent: address(agent), positive }), payer);
  }

  /** The registered agent at `of`, if any, with its reputation. */
  async agent(of: string): Promise<AgentRecord | undefined> {
    const info = await this.api.query.AgentRegistry.Agents.getValue(address(of), { at: "best" });
    return info ? this.record(address(of), info) : undefined;
  }

  /** Registered agents, optionally only the active ones offering `capability`, best rated first. */
  async findAgents(capability?: string, activeOnly = true): Promise<AgentRecord[]> {
    const entries = await this.api.query.AgentRegistry.Agents.getEntries({ at: "best" });
    const records = await Promise.all(
      entries
        .filter(({ value }: { value: { active: boolean } }) => !activeOnly || value.active)
        .map(({ keyArgs, value }: { keyArgs: [string]; value: unknown }) => this.record(address(keyArgs[0]), value)),
    );
    return records
      .filter((r) => !capability || r.profile.capabilities.includes(capability))
      .sort((a, b) => {
        const score = (r: AgentRecord) => r.reputation.positive - r.reputation.negative;
        return (
          score(b) - score(a) ||
          b.reputation.payers - a.reputation.payers ||
          (b.reputation.volume.planck > a.reputation.volume.planck ? 1 : -1)
        );
      });
  }

  private async record(of: string, info: any): Promise<AgentRecord> {
    const r = await this.api.query.AgentRegistry.Reputations.getValue(of, { at: "best" });
    return {
      address: of,
      owner: info.owner ? address(info.owner) : undefined,
      ownerConfirmed: info.owner_confirmed,
      active: info.active,
      registeredAt: info.registered_at,
      profile: {
        name: text(info.profile.name),
        description: text(info.profile.description),
        endpoint: text(info.profile.endpoint),
        capabilities: info.profile.capabilities.map(text),
        metadata: info.profile.metadata
          ? typeof info.profile.metadata === "string"
            ? info.profile.metadata
            : toHex(info.profile.metadata)
          : undefined,
      },
      reputation: {
        payments: BigInt(r.payments),
        volume: new Amount(r.volume),
        payers: r.payers,
        positive: r.positive,
        negative: r.negative,
        tasksCompleted: r.tasks_completed,
        tasksFailed: r.tasks_failed,
      },
    };
  }
}


const encodePolicy = (policy: Policy) => ({
  budget: policy.budget.planck,
  period: policy.periodBlocks ?? 4_800,
  max_per_call: policy.maxPerCall.planck,
  expires_at: policy.expiresAt,
  permissions: { transfers: policy.transfers ?? true, contracts: policy.contracts ?? false },
  targets: (policy.targets ?? []).map(address),
});

const encodeProfile = (profile: Profile) => {
  const capabilities = profile.capabilities ?? [];
  if (capabilities.length > 16) throw new RangeError("at most 16 capabilities");
  // Fixed-size byte arrays are hexadecimal strings for polkadot-api.
  let metadata: string | undefined;
  if (profile.metadata !== undefined) {
    const bytes = fromHex(profile.metadata);
    if (bytes.length !== 32) throw new RangeError("metadata must be a 32-byte hash");
    metadata = toHex(bytes);
  }
  return {
    name: bounded(profile.name, 64, "name"),
    description: bounded(profile.description ?? "", 512, "description"),
    endpoint: bounded(profile.endpoint ?? "", 256, "endpoint"),
    capabilities: capabilities.map((c) => bounded(c, 32, "capability")),
    metadata,
  };
};
