/** Task market: work ordered between agents, with the reward held in escrow. See
 * `docs/agents.md`. */

import { Amount } from "./amount.js";
import type { TesseraxClient, TxOutcome } from "./client.js";
import { TransactionFailed } from "./client.js";
import { formatAddress, Keypair, parseAddress } from "./keys.js";

/** Blocks of 18 seconds covering `minutes`. */
export const blocksFor = (minutes: number) => Math.ceil((minutes * 60) / 18);

/** A task to create. */
export interface NewTask {
  /** What to do: a URI or a short text, at most 256 bytes. */
  spec: string;
  /** Capability tag of the work, such as "translation". */
  capability?: string;
  /** Reward, held in escrow until the task ends. */
  reward: Amount;
  /** The only worker allowed to accept. */
  invited?: string;
  /** Arbiter of disputes and its fee. Without arbiter, results are approved by the client or
   * automatically after the review period. */
  arbiter?: { address: string; fee: Amount };
  /** Bond the worker holds, lost if it does not deliver in time. */
  bond?: Amount;
  /** Blocks the worker has to deliver after accepting. */
  deliveryBlocks: number;
  /** Blocks the client has to approve or dispute after the delivery. */
  reviewBlocks: number;
}

export type TaskStatus =
  | { state: "open" }
  | { state: "assigned"; worker: string; deadline: number }
  | { state: "delivered"; worker: string; result: string; reviewUntil: number }
  | { state: "disputed"; worker: string; result: string; until: number };

/** A task in progress. */
export interface TaskInfo {
  id: bigint;
  client: string;
  spec: string;
  capability?: string;
  reward: Amount;
  invited?: string;
  arbiter?: { address: string; fee: Amount };
  bond: Amount;
  deliveryBlocks: number;
  reviewBlocks: number;
  status: TaskStatus;
}

const utf8 = new TextEncoder();
const text = (bytes: Uint8Array) => new TextDecoder().decode(bytes);
const bounded = (value: string, limit: number, name: string) => {
  const bytes = utf8.encode(value);
  if (bytes.length > limit) throw new RangeError(`${name} is longer than ${limit} bytes`);
  return bytes;
};
const address = (value: string) => formatAddress(parseAddress(value));

/** Task calls and queries, on top of a client. Every call takes an optional owner to act through
 * the signer's delegation. */
export class Tasks {
  constructor(readonly client: TesseraxClient) {}

  private get api() {
    return this.client.api;
  }

  private send(signer: Keypair, owner: string | undefined, call: unknown): Promise<TxOutcome> {
    return owner
      ? this.client.agentExecute(signer, owner, call as { decodedCall: unknown })
      : this.client.submit(call as { createSubmitAndWatch: Function }, signer);
  }

  /** Create a task; returns its id. */
  async create(signer: Keypair, owner: string | undefined, task: NewTask): Promise<bigint> {
    const call = this.api.tx.Tasks.create({
      spec: bounded(task.spec, 256, "spec"),
      capability: task.capability === undefined ? undefined : bounded(task.capability, 32, "capability"),
      reward: task.reward.planck,
      invited: task.invited === undefined ? undefined : address(task.invited),
      arbiter: task.arbiter ? { account: address(task.arbiter.address), fee: task.arbiter.fee.planck } : undefined,
      bond: task.bond?.planck ?? 0n,
      delivery_period: task.deliveryBlocks,
      review_period: task.reviewBlocks,
    });
    const outcome = await this.send(signer, owner, call);
    const event = outcome.events.find((e) => e.type === "Tasks" && e.value.type === "Created");
    if (!event) throw new TransactionFailed("the task was not created");
    return BigInt((event.value.value as { id: bigint }).id);
  }

  cancel(signer: Keypair, owner: string | undefined, id: bigint) {
    return this.send(signer, owner, this.api.tx.Tasks.cancel({ id }));
  }

  accept(signer: Keypair, owner: string | undefined, id: bigint) {
    return this.send(signer, owner, this.api.tx.Tasks.accept({ id }));
  }

  deliver(signer: Keypair, owner: string | undefined, id: bigint, result: string) {
    return this.send(signer, owner, this.api.tx.Tasks.deliver({ id, result: bounded(result, 256, "result") }));
  }

  approve(signer: Keypair, owner: string | undefined, id: bigint) {
    return this.send(signer, owner, this.api.tx.Tasks.approve({ id }));
  }

  dispute(signer: Keypair, owner: string | undefined, id: bigint) {
    return this.send(signer, owner, this.api.tx.Tasks.dispute({ id }));
  }

  /** Decide a dispute as the arbiter: `workerPercent` of the reward goes to the worker. */
  resolve(arbiter: Keypair, id: bigint, workerPercent: number) {
    if (!Number.isInteger(workerPercent) || workerPercent < 0 || workerPercent > 100) {
      throw new RangeError("a share is an integer from 0 to 100");
    }
    return this.send(arbiter, undefined, this.api.tx.Tasks.resolve({ id, worker_share: workerPercent }));
  }

  /** Finalize a task whose time is up; anyone can call it. */
  finalize(signer: Keypair, id: bigint) {
    return this.send(signer, undefined, this.api.tx.Tasks.finalize({ id }));
  }

  async task(id: bigint): Promise<TaskInfo | undefined> {
    const t = await this.api.query.Tasks.Tasks.getValue(id, { at: "best" });
    return t ? info(id, t) : undefined;
  }

  /** Tasks in progress, newest first. */
  async all(): Promise<TaskInfo[]> {
    const entries = await this.api.query.Tasks.Tasks.getEntries({ at: "best" });
    return entries
      .map(({ keyArgs, value }: { keyArgs: [bigint]; value: unknown }) => info(keyArgs[0], value))
      .sort((a: TaskInfo, b: TaskInfo) => (b.id > a.id ? 1 : -1));
  }

  /** Open tasks, optionally only those of `capability`, best paid first. */
  async open(capability?: string): Promise<TaskInfo[]> {
    return (await this.all())
      .filter((t) => t.status.state === "open" && (!capability || t.capability === capability))
      .sort((a, b) => (b.reward.planck > a.reward.planck ? 1 : -1));
  }
}

const info = (id: bigint, t: any): TaskInfo => {
  const s = t.status;
  let status: TaskStatus;
  switch (s.type) {
    case "Assigned":
      status = { state: "assigned", worker: address(s.value.worker), deadline: s.value.deadline };
      break;
    case "Delivered":
      status = { state: "delivered", worker: address(s.value.worker), result: text(s.value.result), reviewUntil: s.value.review_until };
      break;
    case "Disputed":
      status = { state: "disputed", worker: address(s.value.worker), result: text(s.value.result), until: s.value.until };
      break;
    default:
      status = { state: "open" };
  }
  return {
    id: BigInt(id),
    client: address(t.client),
    spec: text(t.spec),
    capability: t.capability ? text(t.capability) : undefined,
    reward: new Amount(t.reward),
    invited: t.invited ? address(t.invited) : undefined,
    arbiter: t.arbiter ? { address: address(t.arbiter.account), fee: new Amount(t.arbiter.fee) } : undefined,
    bond: new Amount(t.bond),
    deliveryBlocks: t.delivery_period,
    reviewBlocks: t.review_period,
    status,
  };
};
