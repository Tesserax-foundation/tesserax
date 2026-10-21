/** Entities a block touches (accounts, agents, delegations, channels, tasks), and their refresh
 * from the chain state: the indexer reads their state again instead of replaying the logic of
 * the pallets, which also makes reorganizations simple. */

import { blake2_256, fromHex, type Runtime } from "./chain.js";
import * as db from "./db.js";
import type { EventRow } from "./db.js";
import type { Rpc } from "./rpc.js";

const ADDRESS = /^te[1-9A-HJ-NP-Za-km-z]{44,48}$/;

export class Touched {
  readonly accounts = new Set<string>();
  readonly agents = new Set<string>();
  readonly delegations = new Set<string>();
  readonly channels = new Set<number>();
  readonly tasks = new Set<number>();

  get size(): number {
    return this.accounts.size + this.agents.size + this.delegations.size + this.channels.size + this.tasks.size;
  }

  account(address: string | null | undefined): void {
    if (address && ADDRESS.test(address)) this.accounts.add(address);
  }

  /** Every address in a decoded value. */
  private addresses(value: unknown): void {
    if (typeof value === "string") this.account(value);
    else if (Array.isArray(value)) value.forEach((v) => this.addresses(v));
    else if (value && typeof value === "object") Object.values(value).forEach((v) => this.addresses(v));
  }

  event(e: Pick<EventRow, "pallet" | "name" | "data">): void {
    const data = e.data as Record<string, any>;
    this.addresses(data);
    switch (e.pallet) {
      case "AgentRegistry":
        if (data.agent) this.agents.add(data.agent);
        break;
      case "Agents":
        if (data.owner && data.agent) this.delegations.add(`${data.owner}/${data.agent}`);
        break;
      case "Channels":
        if (data.id != null) this.channels.add(Number(data.id));
        break;
      case "Tasks":
        if (data.id != null) this.tasks.add(Number(data.id));
        break;
    }
  }

  merge(other: Touched): void {
    other.accounts.forEach((a) => this.accounts.add(a));
    other.agents.forEach((a) => this.agents.add(a));
    other.delegations.forEach((d) => this.delegations.add(d));
    other.channels.forEach((c) => this.channels.add(c));
    other.tasks.forEach((t) => this.tasks.add(t));
  }
}

const text = (bytes: unknown): string =>
  bytes instanceof Uint8Array ? Buffer.from(bytes).toString("utf8") : typeof bytes === "string" ? bytes : "";

/** Values of storage `keys` at block `at`, by key. */
export async function readStorage(rpc: Rpc, keys: string[], at: string): Promise<Map<string, string | null>> {
  const values = new Map<string, string | null>();
  for (let i = 0; i < keys.length; i += 500) {
    const chunk = keys.slice(i, i + 500);
    const result = await rpc.request<Array<{ changes: Array<[string, string | null]> }>>("state_queryStorageAt", [chunk, at]);
    for (const [key, value] of result[0]?.changes ?? []) values.set(key, value);
  }
  return values;
}

/** All the keys under `prefix` at block `at`. */
export async function storageKeys(rpc: Rpc, prefix: string, at: string): Promise<string[]> {
  const keys: string[] = [];
  let start: string | null = null;
  for (;;) {
    const page: string[] = await rpc.request<string[]>("state_getKeysPaged", [prefix, 1000, start ?? prefix, at]);
    keys.push(...page);
    if (page.length < 1000) return keys;
    start = page[page.length - 1];
  }
}

/** Read the touched entities at block `at` (number `block`) and write them to the database. */
export async function refresh(
  rpc: Rpc,
  sql: db.Sql,
  runtime: Runtime,
  touched: Touched,
  at: string,
  block: number,
): Promise<void> {
  if (touched.size === 0) return;
  const keyOf = new Map<string, string>();
  const want = (key: string, entity: string) => keyOf.set(entity, key);
  for (const a of touched.accounts) want(runtime.storageKey("System", "Account", a), `account:${a}`);
  for (const a of touched.agents) {
    want(runtime.storageKey("AgentRegistry", "Agents", a), `agent:${a}`);
    want(runtime.storageKey("AgentRegistry", "Reputations", a), `reputation:${a}`);
  }
  for (const d of touched.delegations) {
    const [owner, agent] = d.split("/");
    want(runtime.storageKey("Agents", "Delegations", owner, agent), `delegation:${d}`);
  }
  for (const c of touched.channels) want(runtime.storageKey("Channels", "Channels", BigInt(c)), `channel:${c}`);
  for (const t of touched.tasks) want(runtime.storageKey("Tasks", "Tasks", BigInt(t)), `task:${t}`);

  const values = await readStorage(rpc, [...new Set(keyOf.values())], at);
  const value = (entity: string) => values.get(keyOf.get(entity)!) ?? null;

  const accounts: db.AccountRow[] = [];
  for (const address of touched.accounts) {
    const info = runtime.decodeStorage("System", "Account", value(`account:${address}`));
    accounts.push({
      address,
      free: BigInt(info.data.free),
      reserved: BigInt(info.data.reserved),
      frozen: BigInt(info.data.frozen),
      nonce: Number(info.nonce),
    });
  }
  await db.upsertAccounts(sql, accounts, block);

  for (const address of touched.agents) {
    const raw = value(`agent:${address}`);
    if (raw == null) {
      await db.deregisterAgent(sql, address, block);
      continue;
    }
    const info = runtime.decodeStorage("AgentRegistry", "Agents", raw);
    const r = runtime.decodeStorage("AgentRegistry", "Reputations", value(`reputation:${address}`));
    await db.upsertAgent(
      sql,
      {
        address,
        owner: info.owner ?? null,
        ownerConfirmed: Boolean(info.owner_confirmed),
        name: text(info.profile.name),
        description: text(info.profile.description),
        endpoint: text(info.profile.endpoint),
        capabilities: (info.profile.capabilities as unknown[]).map(text),
        metadata: info.profile.metadata ?? null,
        active: Boolean(info.active),
        registeredAt: Number(info.registered_at),
        deposit: BigInt(info.deposit),
        reputation: {
          payments: BigInt(r?.payments ?? 0),
          volume: BigInt(r?.volume ?? 0),
          payers: BigInt(r?.payers ?? 0),
          positive: BigInt(r?.positive ?? 0),
          negative: BigInt(r?.negative ?? 0),
          tasksCompleted: BigInt(r?.tasks_completed ?? 0),
          tasksFailed: BigInt(r?.tasks_failed ?? 0),
        },
      },
      block,
    );
  }

  for (const d of touched.delegations) {
    const [owner, agent] = d.split("/");
    const raw = value(`delegation:${d}`);
    if (raw == null) {
      await db.revokeDelegation(sql, owner, agent, block);
      continue;
    }
    const delegation = runtime.decodeStorage("Agents", "Delegations", raw);
    const policy = delegation.policy;
    await db.upsertDelegation(
      sql,
      {
        owner,
        agent,
        budget: BigInt(policy.budget),
        period: Number(policy.period),
        maxPerCall: BigInt(policy.max_per_call),
        expiresAt: policy.expires_at == null ? null : Number(policy.expires_at),
        transfers: Boolean(policy.permissions.transfers),
        contracts: Boolean(policy.permissions.contracts),
        targets: policy.targets as string[],
        spent: BigInt(delegation.spent),
        periodStart: Number(delegation.period_start),
      },
      block,
    );
  }

  /** The storage value of `key` just before the block `end`: for an entity created and removed
   * since the last refresh, its last state. */
  const before = async (key: string, end: { hash: string } | null): Promise<string | null> => {
    if (!end) return null;
    const header = await rpc.request<{ parentHash: string }>("chain_getHeader", [end.hash]);
    return (await readStorage(rpc, [key], header.parentHash)).get(key) ?? null;
  };

  for (const id of touched.channels) {
    let raw = value(`channel:${id}`);
    if (raw == null) {
      if (!(await db.exists(sql, "channels", id))) {
        raw = await before(keyOf.get(`channel:${id}`)!, await db.lastEvent(sql, "Channels", ["Closed"], id));
        if (raw != null) await upsertChannel(id, raw);
      }
      await db.closeChannel(sql, id, block);
      continue;
    }
    await upsertChannel(id, raw);
  }

  async function upsertChannel(id: number, raw: string) {
    const c = runtime.decodeStorage("Channels", "Channels", raw);
    await db.upsertChannel(
      sql,
      {
        id,
        payer: c.payer,
        payee: c.payee,
        signer: runtime.addressOf(blake2_256(fromHex(c.signer))),
        deposit: BigInt(c.deposit),
        claimed: BigInt(c.claimed),
        closesAt: c.closes_at == null ? null : Number(c.closes_at),
      },
      block,
    );
  }

  for (const id of touched.tasks) {
    let raw = value(`task:${id}`);
    if (raw == null) {
      if (!(await db.exists(sql, "tasks", id))) {
        raw = await before(keyOf.get(`task:${id}`)!, await db.lastEvent(sql, "Tasks", ["Completed", "Cancelled", "Expired"], id));
        if (raw != null) await upsertTask(id, raw);
      }
      await db.finishTask(sql, id, block);
      continue;
    }
    await upsertTask(id, raw);
  }

  async function upsertTask(id: number, raw: string) {
    const t = runtime.decodeStorage("Tasks", "Tasks", raw);
    const status = t.status as { type: string; value?: any };
    await db.upsertTask(
      sql,
      {
        id,
        client: t.client,
        spec: text(t.spec),
        capability: t.capability == null ? null : text(t.capability),
        reward: BigInt(t.reward),
        invited: t.invited ?? null,
        arbiter: t.arbiter?.account ?? null,
        arbiterFee: t.arbiter == null ? null : BigInt(t.arbiter.fee),
        bond: BigInt(t.bond),
        deliveryPeriod: Number(t.delivery_period),
        reviewPeriod: Number(t.review_period),
        status: status.type.toLowerCase() as db.TaskRow["status"],
        worker: status.value?.worker ?? null,
        deadline: status.value?.deadline == null ? null : Number(status.value.deadline),
        result: status.value?.result == null ? null : text(status.value.result),
        reviewUntil:
          status.value?.review_until != null ? Number(status.value.review_until)
          : status.value?.until != null ? Number(status.value.until)
          : null,
      },
      block,
    );
  }
}
