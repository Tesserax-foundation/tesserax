/** Writes of the indexer to the explorer database (Supabase Postgres). */

import postgres from "postgres";

export type Sql = postgres.Sql<{}>;

export function connect(url: string): Sql {
  return postgres(url, {
    max: 4,
    onnotice: () => {},
    // Supabase poolers in transaction mode do not support prepared statements.
    prepare: false,
  });
}

/** Amounts and other big integers go to `numeric` columns as decimal strings. */
const num = (value: bigint | number | null | undefined) => (value == null ? null : value.toString());

export interface ExtrinsicRow {
  index: number;
  hash: string;
  signer: string | null;
  pallet: string;
  call: string;
  args: unknown;
  nonce: number | null;
  tip: bigint | null;
  fee: bigint | null;
  success: boolean;
  error: string | null;
  size: number;
}

export interface EventRow {
  index: number;
  extrinsicIndex: number | null;
  pallet: string;
  name: string;
  data: unknown;
}

export interface TransferRow {
  eventIndex: number;
  extrinsicIndex: number | null;
  extrinsicHash: string | null;
  from: string;
  to: string;
  amount: bigint;
}

export interface BlockRow {
  hash: string;
  number: number;
  parentHash: string;
  stateRoot: string;
  extrinsicsRoot: string;
  author: string | null;
  timestamp: Date | null;
  difficulty: bigint | null;
  powNonce: string | null;
  powHash: string | null;
  reward: bigint;
  fees: bigint;
  size: number;
  specVersion: number;
  extrinsics: ExtrinsicRow[];
  events: EventRow[];
  transfers: TransferRow[];
}

/** The highest canonical block indexed. */
export async function canonicalTip(sql: Sql): Promise<{ number: number; hash: string } | null> {
  const [row] = await sql`select number, hash from blocks where canonical order by number desc limit 1`;
  return row ? { number: Number(row.number), hash: row.hash } : null;
}

export async function canonicalHash(sql: Sql, number: number): Promise<string | null> {
  const [row] = await sql`select hash from blocks where canonical and number = ${number}`;
  return row?.hash ?? null;
}

/** Remove the canonical blocks above `number` from the best chain: they stay as orphaned
 * blocks, without their extrinsics, events and transfers. Returns their events, whose
 * entities must be read again. */
export async function orphanAbove(sql: Sql, number: number): Promise<{ hashes: string[]; events: EventRow[] }> {
  return sql.begin(async (tx) => {
    const blocks = await tx`
      update blocks set canonical = false, finalized = false
      where canonical and number > ${number} returning hash`;
    const hashes = blocks.map((b) => b.hash as string);
    if (hashes.length === 0) return { hashes, events: [] };
    const events = await tx`
      delete from events where block_hash in ${tx(hashes)}
      returning index, extrinsic_index, pallet, name, data`;
    await tx`delete from extrinsics where block_hash in ${tx(hashes)}`;
    await tx`delete from transfers where block_hash in ${tx(hashes)}`;
    return {
      hashes,
      events: events.map((e) => ({
        index: e.index,
        extrinsicIndex: e.extrinsic_index,
        pallet: e.pallet,
        name: e.name,
        data: e.data,
      })),
    };
  }) as Promise<{ hashes: string[]; events: EventRow[] }>;
}

/** Insert `block` as the canonical block at its height, with its extrinsics, events and
 * transfers, in one transaction. */
export async function saveBlock(sql: Sql, block: BlockRow): Promise<void> {
  const timestamp = block.timestamp;
  await sql.begin(async (tx) => {
    // A block indexed before and orphaned since may become canonical again.
    await tx`delete from extrinsics where block_hash = ${block.hash}`;
    await tx`delete from events where block_hash = ${block.hash}`;
    await tx`delete from transfers where block_hash = ${block.hash}`;
    await tx`
      insert into blocks (hash, number, parent_hash, state_root, extrinsics_root, author, timestamp,
        difficulty, pow_nonce, pow_hash, extrinsic_count, event_count, transfer_count, reward, fees,
        size, spec_version, canonical, finalized)
      values (${block.hash}, ${block.number}, ${block.parentHash}, ${block.stateRoot},
        ${block.extrinsicsRoot}, ${block.author}, ${timestamp}, ${num(block.difficulty)},
        ${block.powNonce}, ${block.powHash}, ${block.extrinsics.length}, ${block.events.length},
        ${block.transfers.length}, ${num(block.reward)}, ${num(block.fees)}, ${block.size},
        ${block.specVersion}, true, false)
      on conflict (hash) do update set canonical = true, extrinsic_count = excluded.extrinsic_count,
        event_count = excluded.event_count, transfer_count = excluded.transfer_count,
        reward = excluded.reward, fees = excluded.fees, indexed_at = now()`;
    await insertChunks(
      tx,
      "extrinsics",
      block.extrinsics.map((x) => ({
        block_hash: block.hash,
        block_number: block.number,
        index: x.index,
        hash: x.hash,
        signer: x.signer,
        pallet: x.pallet,
        call: x.call,
        args: tx.json(x.args as postgres.JSONValue),
        nonce: x.nonce,
        tip: num(x.tip),
        fee: num(x.fee),
        success: x.success,
        error: x.error,
        size: x.size,
        timestamp,
      })),
    );
    await insertChunks(
      tx,
      "events",
      block.events.map((e) => ({
        block_hash: block.hash,
        block_number: block.number,
        index: e.index,
        extrinsic_index: e.extrinsicIndex,
        pallet: e.pallet,
        name: e.name,
        data: tx.json(e.data as postgres.JSONValue),
        timestamp,
      })),
    );
    await insertChunks(
      tx,
      "transfers",
      block.transfers.map((t) => ({
        block_hash: block.hash,
        block_number: block.number,
        event_index: t.eventIndex,
        extrinsic_index: t.extrinsicIndex,
        extrinsic_hash: t.extrinsicHash,
        from_address: t.from,
        to_address: t.to,
        amount: num(t.amount),
        timestamp,
      })),
    );
  });
}

/** Insert `rows` into `table`, a statement per 1000 rows (Postgres takes 65535 parameters). */
async function insertChunks(sql: postgres.TransactionSql, table: string, rows: Record<string, unknown>[]) {
  for (let i = 0; i < rows.length; i += 1000) {
    const chunk = rows.slice(i, i + 1000);
    await sql`insert into ${sql(table)} ${sql(chunk as any)}`;
  }
}

/** Mark the canonical blocks up to `number` as finalized. */
export async function markFinalized(sql: Sql, number: number): Promise<void> {
  await sql`update blocks set finalized = true where canonical and not finalized and number <= ${number}`;
}

export async function setState(sql: Sql, key: string, value: unknown): Promise<void> {
  await sql`
    insert into chain_state (key, value, updated_at) values (${key}, ${sql.json(value as postgres.JSONValue)}, now())
    on conflict (key) do update set value = excluded.value, updated_at = now()`;
}

export interface AccountRow {
  address: string;
  free: bigint;
  reserved: bigint;
  frozen: bigint;
  nonce: number;
}

export async function upsertAccounts(sql: Sql, rows: AccountRow[], block: number): Promise<void> {
  for (const a of rows) {
    await sql`
      insert into accounts (address, free, reserved, frozen, nonce, first_seen_block, updated_block)
      values (${a.address}, ${num(a.free)}, ${num(a.reserved)}, ${num(a.frozen)}, ${a.nonce}, ${block}, ${block})
      on conflict (address) do update set free = excluded.free, reserved = excluded.reserved,
        frozen = excluded.frozen, nonce = excluded.nonce, updated_block = excluded.updated_block`;
  }
}

export interface AgentRow {
  address: string;
  owner: string | null;
  ownerConfirmed: boolean;
  name: string;
  description: string;
  endpoint: string;
  capabilities: string[];
  metadata: string | null;
  active: boolean;
  registeredAt: number;
  deposit: bigint;
  reputation: {
    payments: bigint;
    volume: bigint;
    payers: bigint;
    positive: bigint;
    negative: bigint;
    tasksCompleted: bigint;
    tasksFailed: bigint;
  };
}

export async function upsertAgent(sql: Sql, a: AgentRow, block: number): Promise<void> {
  const r = a.reputation;
  await sql`
    insert into agents (address, owner, owner_confirmed, name, description, endpoint, capabilities,
      metadata, active, registered_at, deposit, payments, volume, payers, positive, negative,
      tasks_completed, tasks_failed, deregistered, updated_block)
    values (${a.address}, ${a.owner}, ${a.ownerConfirmed}, ${a.name}, ${a.description}, ${a.endpoint},
      ${a.capabilities}, ${a.metadata}, ${a.active}, ${a.registeredAt}, ${num(a.deposit)},
      ${num(r.payments)}, ${num(r.volume)}, ${num(r.payers)}, ${num(r.positive)}, ${num(r.negative)},
      ${num(r.tasksCompleted)}, ${num(r.tasksFailed)}, false, ${block})
    on conflict (address) do update set owner = excluded.owner, owner_confirmed = excluded.owner_confirmed,
      name = excluded.name, description = excluded.description, endpoint = excluded.endpoint,
      capabilities = excluded.capabilities, metadata = excluded.metadata, active = excluded.active,
      registered_at = excluded.registered_at, deposit = excluded.deposit, payments = excluded.payments,
      volume = excluded.volume, payers = excluded.payers, positive = excluded.positive,
      negative = excluded.negative, tasks_completed = excluded.tasks_completed,
      tasks_failed = excluded.tasks_failed, deregistered = false, updated_block = excluded.updated_block`;
}

export async function deregisterAgent(sql: Sql, address: string, block: number): Promise<void> {
  await sql`update agents set deregistered = true, active = false, updated_block = ${block} where address = ${address}`;
}

export interface DelegationRow {
  owner: string;
  agent: string;
  budget: bigint;
  period: number;
  maxPerCall: bigint;
  expiresAt: number | null;
  transfers: boolean;
  contracts: boolean;
  targets: string[];
  spent: bigint;
  periodStart: number;
}

export async function upsertDelegation(sql: Sql, d: DelegationRow, block: number): Promise<void> {
  await sql`
    insert into delegations (owner, agent, budget, period, max_per_call, expires_at, transfers, contracts,
      targets, spent, period_start, active, updated_block)
    values (${d.owner}, ${d.agent}, ${num(d.budget)}, ${d.period}, ${num(d.maxPerCall)}, ${d.expiresAt},
      ${d.transfers}, ${d.contracts}, ${d.targets}, ${num(d.spent)}, ${d.periodStart}, true, ${block})
    on conflict (owner, agent) do update set budget = excluded.budget, period = excluded.period,
      max_per_call = excluded.max_per_call, expires_at = excluded.expires_at, transfers = excluded.transfers,
      contracts = excluded.contracts, targets = excluded.targets, spent = excluded.spent,
      period_start = excluded.period_start, active = true, updated_block = excluded.updated_block`;
}

export async function revokeDelegation(sql: Sql, owner: string, agent: string, block: number): Promise<void> {
  await sql`update delegations set active = false, updated_block = ${block} where owner = ${owner} and agent = ${agent}`;
}

export interface ChannelRow {
  id: number;
  payer: string;
  payee: string;
  signer: string;
  deposit: bigint;
  claimed: bigint;
  closesAt: number | null;
}

export async function upsertChannel(sql: Sql, c: ChannelRow, block: number): Promise<void> {
  await sql`
    insert into channels (id, payer, payee, signer, deposit, claimed, closes_at, status, opened_block, updated_block)
    values (${c.id}, ${c.payer}, ${c.payee}, ${c.signer}, ${num(c.deposit)}, ${num(c.claimed)}, ${c.closesAt},
      ${c.closesAt == null ? "open" : "closing"},
      (select min(block_number) from events where pallet = 'Channels' and name = 'Opened' and (data->>'id')::bigint = ${c.id}),
      ${block})
    on conflict (id) do update set deposit = excluded.deposit, claimed = excluded.claimed,
      closes_at = excluded.closes_at, status = excluded.status, opened_block = excluded.opened_block,
      closed_block = null, refunded = null, updated_block = excluded.updated_block`;
}

/** A channel no longer in storage: closed if its Closed event is canonical, else it was only
 * opened in orphaned blocks. */
export async function closeChannel(sql: Sql, id: number, block: number): Promise<void> {
  const [closed] = await sql`
    select block_number, data->>'refunded' as refunded from events
    where pallet = 'Channels' and name = 'Closed' and (data->>'id')::bigint = ${id}
    order by block_number desc limit 1`;
  if (closed) {
    await sql`
      update channels set status = 'closed', closed_block = ${closed.block_number}, refunded = ${closed.refunded},
        closes_at = null, updated_block = ${block}
      where id = ${id}`;
  } else {
    await sql`delete from channels where id = ${id}`;
  }
}

export interface TaskRow {
  id: number;
  client: string;
  spec: string;
  capability: string | null;
  reward: bigint;
  invited: string | null;
  arbiter: string | null;
  arbiterFee: bigint | null;
  bond: bigint;
  deliveryPeriod: number;
  reviewPeriod: number;
  status: "open" | "assigned" | "delivered" | "disputed";
  worker: string | null;
  deadline: number | null;
  result: string | null;
  reviewUntil: number | null;
}

export async function upsertTask(sql: Sql, t: TaskRow, block: number): Promise<void> {
  await sql`
    insert into tasks (id, client, spec, capability, reward, invited, arbiter, arbiter_fee, bond,
      delivery_period, review_period, status, worker, deadline, result, review_until, created_block, updated_block)
    values (${t.id}, ${t.client}, ${t.spec}, ${t.capability}, ${num(t.reward)}, ${t.invited}, ${t.arbiter},
      ${num(t.arbiterFee)}, ${num(t.bond)}, ${t.deliveryPeriod}, ${t.reviewPeriod}, ${t.status}, ${t.worker},
      ${t.deadline}, ${t.result}, ${t.reviewUntil},
      (select min(block_number) from events where pallet = 'Tasks' and name = 'Created' and (data->>'id')::bigint = ${t.id}),
      ${block})
    on conflict (id) do update set status = excluded.status, worker = excluded.worker,
      deadline = excluded.deadline, result = excluded.result, review_until = excluded.review_until,
      reward = excluded.reward, created_block = excluded.created_block, paid = null, refunded = null,
      updated_block = excluded.updated_block`;
}

/** A task no longer in storage: its final state comes from its last canonical event. */
export async function finishTask(sql: Sql, id: number, block: number): Promise<void> {
  const [last] = await sql`
    select name, data from events
    where pallet = 'Tasks' and name in ('Completed', 'Cancelled', 'Expired') and (data->>'id')::bigint = ${id}
    order by block_number desc, index desc limit 1`;
  if (!last) {
    await sql`delete from tasks where id = ${id}`;
    return;
  }
  const status = (last.name as string).toLowerCase();
  const data = last.data as Record<string, string>;
  await sql`
    update tasks set status = ${status}, worker = coalesce(${data.worker ?? null}, worker),
      paid = ${data.paid ?? null}, refunded = ${data.refunded ?? null}, updated_block = ${block}
    where id = ${id}`;
}
