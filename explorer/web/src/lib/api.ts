/** Queries of the explorer database. Amounts come back as planck strings (`::text`), as JSON
 * numbers would lose precision. */

import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { supabase } from "./supabase";

export const PAGE_SIZE = 25;

async function run<T>(query: PromiseLike<{ data: unknown; error: { message: string } | null }>): Promise<T> {
  const { data, error } = await query;
  if (error) throw new Error(error.message);
  return data as T;
}

/** A page of rows, and whether there is a next one (one more row is fetched to know). */
export interface Page<T> {
  rows: T[];
  more: boolean;
}

const page = async <T>(query: any, index: number, size = PAGE_SIZE): Promise<Page<T>> => {
  const rows = await run<T[]>(query.range(index * size, index * size + size));
  return { rows: rows.slice(0, size), more: rows.length > size };
};

// ---------------------------------------------------------------------------------------------
// Types

export interface Block {
  number: number;
  hash: string;
  parent_hash: string;
  state_root: string;
  extrinsics_root: string;
  author: string | null;
  timestamp: string | null;
  difficulty: string | null;
  pow_nonce: string | null;
  pow_hash: string | null;
  extrinsic_count: number;
  event_count: number;
  transfer_count: number;
  reward: string;
  fees: string;
  size: number;
  spec_version: number;
  canonical: boolean;
  finalized: boolean;
}

export interface Extrinsic {
  block_hash: string;
  block_number: number;
  index: number;
  hash: string;
  signer: string | null;
  pallet: string;
  call: string;
  args: unknown;
  nonce: number | null;
  tip: string | null;
  fee: string | null;
  success: boolean;
  error: string | null;
  size: number;
  timestamp: string | null;
}

export interface ChainEvent {
  block_hash: string;
  block_number: number;
  index: number;
  extrinsic_index: number | null;
  pallet: string;
  name: string;
  data: Record<string, unknown>;
  timestamp: string | null;
}

export interface Transfer {
  block_hash: string;
  block_number: number;
  event_index: number;
  extrinsic_index: number | null;
  extrinsic_hash: string | null;
  from_address: string;
  to_address: string;
  amount: string;
  timestamp: string | null;
}

export interface Account {
  address: string;
  free: string;
  reserved: string;
  frozen: string;
  nonce: number;
  first_seen_block: number | null;
  updated_block: number;
}

export interface Agent {
  address: string;
  owner: string | null;
  owner_confirmed: boolean;
  name: string;
  description: string;
  endpoint: string;
  capabilities: string[];
  metadata: string | null;
  active: boolean;
  registered_at: number | null;
  deposit: string;
  payments: number;
  volume: string;
  payers: number;
  positive: number;
  negative: number;
  tasks_completed: number;
  tasks_failed: number;
  deregistered: boolean;
  updated_block: number;
}

export interface Delegation {
  owner: string;
  agent: string;
  budget: string;
  period: number;
  max_per_call: string;
  expires_at: number | null;
  transfers: boolean;
  contracts: boolean;
  targets: string[];
  spent: string;
  period_start: number;
  active: boolean;
  updated_block: number;
}

export interface Channel {
  id: number;
  payer: string;
  payee: string;
  signer: string;
  deposit: string;
  claimed: string;
  closes_at: number | null;
  status: "open" | "closing" | "closed";
  refunded: string | null;
  opened_block: number | null;
  closed_block: number | null;
  updated_block: number;
}

export type TaskStatus = "open" | "assigned" | "delivered" | "disputed" | "completed" | "cancelled" | "expired";

export interface Task {
  id: number;
  client: string;
  spec: string;
  capability: string | null;
  reward: string;
  invited: string | null;
  arbiter: string | null;
  arbiter_fee: string | null;
  bond: string;
  delivery_period: number;
  review_period: number;
  status: TaskStatus;
  worker: string | null;
  deadline: number | null;
  result: string | null;
  review_until: number | null;
  paid: string | null;
  refunded: string | null;
  created_block: number | null;
  updated_block: number;
}

export interface Overview {
  best: number | null;
  finalized: number | null;
  block_time: number | null;
  hashrate: string | null;
  difficulty: string | null;
  extrinsics_24h: number;
  transfers_24h: number;
  volume_24h: string;
  miners_24h: number;
  accounts: number;
  agents: number;
  open_tasks: number;
  open_channels: number;
  orphaned_24h: number;
}

export interface ChainState {
  head?: { best: number; best_hash: string; node_best: number; finalized: number; spec_version: number; next_difficulty: string };
  supply?: {
    total_issuance: string;
    mined: string;
    mining_supply: string;
    block_reward: string;
    halving_era: number;
    next_halving: number;
    shielded_pool: string;
    shielded_notes: number;
  };
  constants?: {
    token: string;
    decimals: number;
    ss58_prefix: number;
    spec_version: number;
    target_block_time_ms: number;
    difficulty_window: number;
    initial_reward: string;
    halving_interval: number;
    mining_supply: string;
    existential_deposit: string;
    channel_close_period: number;
    task_arbitration_period: number;
    shielded_minimum_fee: string;
  };
  allocations?: Array<{
    account: string;
    amount: string;
    liquid: string;
    cliff: number;
    duration: number;
    vesting: Array<{ locked: string; per_block: string; starting_block: number }>;
  }>;
  updated_at?: string;
}

export interface DailyStat {
  day: string;
  blocks: number;
  extrinsics: number;
  transfers: number;
  volume: string;
  avg_block_time: number | null;
  avg_difficulty: string | null;
  miners: number;
  rewards: string;
  fees: string;
}

export interface Miner {
  author: string;
  blocks: number;
  rewards: string;
  fees: string;
  share: number;
  last_block: number;
}

export interface RichEntry {
  address: string;
  free: string;
  reserved: string;
  frozen: string;
  total: string;
  nonce: number;
  share: number | null;
}

export interface SearchResult {
  kind: "block" | "extrinsic" | "account" | "agent" | "task" | "channel";
  id: string;
  label: string;
}

// ---------------------------------------------------------------------------------------------
// Columns, with amounts as text.

const BLOCK = "number, hash, parent_hash, state_root, extrinsics_root, author, timestamp, difficulty::text, pow_nonce, pow_hash, extrinsic_count, event_count, transfer_count, reward::text, fees::text, size, spec_version, canonical, finalized";
const EXTRINSIC = "block_hash, block_number, index, hash, signer, pallet, call, args, nonce, tip::text, fee::text, success, error, size, timestamp";
const EXTRINSIC_SHORT = "block_hash, block_number, index, hash, signer, pallet, call, nonce, fee::text, success, error, size, timestamp";
const TRANSFER = "block_hash, block_number, event_index, extrinsic_index, extrinsic_hash, from_address, to_address, amount::text, timestamp";
const ACCOUNT = "address, free::text, reserved::text, frozen::text, nonce, first_seen_block, updated_block";
const AGENT = "address, owner, owner_confirmed, name, description, endpoint, capabilities, metadata, active, registered_at, deposit::text, payments, volume::text, payers, positive, negative, tasks_completed, tasks_failed, deregistered, updated_block";
const DELEGATION = "owner, agent, budget::text, period, max_per_call::text, expires_at, transfers, contracts, targets, spent::text, period_start, active, updated_block";
const CHANNEL = "id, payer, payee, signer, deposit::text, claimed::text, closes_at, status, refunded::text, opened_block, closed_block, updated_block";
const TASK = "id, client, spec, capability, reward::text, invited, arbiter, arbiter_fee::text, bond::text, delivery_period, review_period, status, worker, deadline, result, review_until, paid::text, refunded::text, created_block, updated_block";

// ---------------------------------------------------------------------------------------------
// Network

export const useOverview = () =>
  useQuery({
    queryKey: ["overview"],
    queryFn: () => run<Overview>(supabase.rpc("explorer_overview")),
    refetchInterval: 30_000,
  });

export const useChainState = () =>
  useQuery({
    queryKey: ["chain-state"],
    queryFn: async () => {
      const rows = await run<Array<{ key: string; value: any; updated_at: string }>>(
        supabase.from("chain_state").select("key, value, updated_at"),
      );
      const state: ChainState = {};
      for (const row of rows) {
        (state as any)[row.key] = row.value;
        if (row.key === "head") state.updated_at = row.updated_at;
      }
      return state;
    },
    refetchInterval: 30_000,
  });

export const useDailyStats = (days = 30) =>
  useQuery({
    queryKey: ["daily", days],
    queryFn: () => run<DailyStat[]>(supabase.rpc("daily_stats", { days })),
    staleTime: 60_000,
  });

export const useTopMiners = (since: string) =>
  useQuery({
    queryKey: ["miners", since],
    queryFn: () => run<Miner[]>(supabase.rpc("top_miners", { since, max_rows: 100 })),
  });

export const useRichList = (index: number) =>
  useQuery({
    queryKey: ["rich", index],
    queryFn: async (): Promise<Page<RichEntry>> => {
      const rows = await run<RichEntry[]>(supabase.rpc("rich_list", { max_rows: PAGE_SIZE + 1, skip: index * PAGE_SIZE }));
      return { rows: rows.slice(0, PAGE_SIZE), more: rows.length > PAGE_SIZE };
    },
    placeholderData: keepPreviousData,
  });

export const useSearch = (query: string) =>
  useQuery({
    queryKey: ["search", query],
    queryFn: () => run<SearchResult[]>(supabase.rpc("explorer_search", { query })),
    enabled: query.trim().length > 0,
  });

// ---------------------------------------------------------------------------------------------
// Blocks

export const useLatestBlocks = (count = 10) =>
  useQuery({
    queryKey: ["blocks", "latest", count],
    queryFn: () => run<Block[]>(supabase.from("blocks").select(BLOCK).eq("canonical", true).order("number", { ascending: false }).limit(count)),
  });

export const useBlocks = (index: number, filter: { author?: string; orphaned?: boolean } = {}) =>
  useQuery({
    queryKey: ["blocks", "page", index, filter],
    queryFn: () => {
      let query = supabase.from("blocks").select(BLOCK).eq("canonical", !filter.orphaned);
      if (filter.author) query = query.eq("author", filter.author);
      return page<Block>(query.order("number", { ascending: false }), index);
    },
    placeholderData: keepPreviousData,
  });

/** A block by number (its canonical block) or hash, with its extrinsics and events. */
export const useBlock = (id: string) =>
  useQuery({
    queryKey: ["block", id],
    queryFn: async () => {
      const byHash = id.startsWith("0x");
      let query = supabase.from("blocks").select(BLOCK);
      query = byHash ? query.eq("hash", id.toLowerCase()) : query.eq("number", Number(id)).eq("canonical", true);
      const [block] = await run<Block[]>(query.limit(1));
      if (!block) return null;
      const [extrinsics, events, siblings] = await Promise.all([
        run<Extrinsic[]>(supabase.from("extrinsics").select(EXTRINSIC).eq("block_hash", block.hash).order("index")),
        run<ChainEvent[]>(supabase.from("events").select("*").eq("block_hash", block.hash).order("index")),
        run<Block[]>(supabase.from("blocks").select(BLOCK).eq("number", block.number).neq("hash", block.hash)),
      ]);
      return { block, extrinsics, events, siblings };
    },
  });

// ---------------------------------------------------------------------------------------------
// Extrinsics and transfers

export const useLatestExtrinsics = (count = 10) =>
  useQuery({
    queryKey: ["extrinsics", "latest", count],
    queryFn: () =>
      run<Extrinsic[]>(
        supabase.from("extrinsics").select(EXTRINSIC_SHORT).not("signer", "is", null).order("block_number", { ascending: false }).order("index", { ascending: false }).limit(count),
      ),
  });

export const useExtrinsics = (index: number, filter: { signer?: string; pallet?: string; call?: string; all?: boolean } = {}) =>
  useQuery({
    queryKey: ["extrinsics", "page", index, filter],
    queryFn: () => {
      let query = supabase.from("extrinsics").select(EXTRINSIC_SHORT);
      if (filter.signer) query = query.eq("signer", filter.signer);
      else if (!filter.all) query = query.not("signer", "is", null);
      if (filter.pallet) query = query.eq("pallet", filter.pallet);
      if (filter.call) query = query.eq("call", filter.call);
      return page<Extrinsic>(query.order("block_number", { ascending: false }).order("index", { ascending: false }), index);
    },
    placeholderData: keepPreviousData,
  });

/** An extrinsic by hash, or by "block-index". */
export const useExtrinsic = (id: string) =>
  useQuery({
    queryKey: ["extrinsic", id],
    queryFn: async () => {
      let rows: Extrinsic[];
      const position = /^(\d+)-(\d+)$/.exec(id);
      if (position) {
        const [block] = await run<Array<{ hash: string }>>(
          supabase.from("blocks").select("hash").eq("number", Number(position[1])).eq("canonical", true).limit(1),
        );
        rows = block ? await run<Extrinsic[]>(supabase.from("extrinsics").select(EXTRINSIC).eq("block_hash", block.hash).eq("index", Number(position[2]))) : [];
      } else {
        rows = await run<Extrinsic[]>(supabase.from("extrinsics").select(EXTRINSIC).eq("hash", id.toLowerCase()).order("block_number", { ascending: false }).limit(1));
      }
      const extrinsic = rows[0];
      if (!extrinsic) return null;
      const [events, blocks] = await Promise.all([
        run<ChainEvent[]>(supabase.from("events").select("*").eq("block_hash", extrinsic.block_hash).eq("extrinsic_index", extrinsic.index).order("index")),
        run<Block[]>(supabase.from("blocks").select(BLOCK).eq("hash", extrinsic.block_hash).limit(1)),
      ]);
      return { extrinsic, events, block: blocks[0] };
    },
  });

export const useLatestTransfers = (count = 10) =>
  useQuery({
    queryKey: ["transfers", "latest", count],
    queryFn: () =>
      run<Transfer[]>(supabase.from("transfers").select(TRANSFER).order("block_number", { ascending: false }).order("event_index", { ascending: false }).limit(count)),
  });

export const useTransfers = (index: number, address?: string) =>
  useQuery({
    queryKey: ["transfers", "page", index, address],
    queryFn: () => {
      let query = supabase.from("transfers").select(TRANSFER);
      if (address) query = query.or(`from_address.eq.${address},to_address.eq.${address}`);
      return page<Transfer>(query.order("block_number", { ascending: false }).order("event_index", { ascending: false }), index);
    },
    placeholderData: keepPreviousData,
  });

export const useEvents = (index: number, filter: { pallet?: string; name?: string } = {}) =>
  useQuery({
    queryKey: ["events", "page", index, filter],
    queryFn: () => {
      let query = supabase.from("events").select("*");
      if (filter.pallet) query = query.eq("pallet", filter.pallet);
      if (filter.name) query = query.eq("name", filter.name);
      return page<ChainEvent>(query.order("block_number", { ascending: false }).order("index", { ascending: false }), index);
    },
    placeholderData: keepPreviousData,
  });

// ---------------------------------------------------------------------------------------------
// Accounts

export const useAccount = (address: string) =>
  useQuery({
    queryKey: ["account", address],
    queryFn: async () => {
      const [accounts, agents, delegated, delegations, mined, sent] = await Promise.all([
        run<Account[]>(supabase.from("accounts").select(ACCOUNT).eq("address", address).limit(1)),
        run<Agent[]>(supabase.from("agents").select(AGENT).eq("address", address).limit(1)),
        run<Delegation[]>(supabase.from("delegations").select(DELEGATION).eq("owner", address).order("updated_block", { ascending: false })),
        run<Delegation[]>(supabase.from("delegations").select(DELEGATION).eq("agent", address).order("updated_block", { ascending: false })),
        supabase.from("blocks").select("hash", { count: "exact", head: true }).eq("canonical", true).eq("author", address),
        supabase.from("extrinsics").select("hash", { count: "exact", head: true }).eq("signer", address),
      ]);
      return {
        account: accounts[0] ?? null,
        agent: agents[0] ?? null,
        delegated,
        delegations,
        minedCount: mined.count ?? 0,
        extrinsicCount: sent.count ?? 0,
      };
    },
  });

export const useOwnedAgents = (owner: string) =>
  useQuery({
    queryKey: ["owned-agents", owner],
    queryFn: () => run<Agent[]>(supabase.from("agents").select(AGENT).eq("owner", owner).order("volume", { ascending: false })),
  });

// ---------------------------------------------------------------------------------------------
// Agents, tasks and channels

export type AgentSort = "volume" | "payments" | "positive" | "registered_at" | "tasks_completed";

export const useAgents = (index: number, filter: { capability?: string; search?: string; sort?: AgentSort; inactive?: boolean } = {}) =>
  useQuery({
    queryKey: ["agents", index, filter],
    queryFn: () => {
      let query = supabase.from("agents").select(AGENT).eq("deregistered", false);
      if (!filter.inactive) query = query.eq("active", true);
      if (filter.capability) query = query.contains("capabilities", [filter.capability]);
      if (filter.search) query = query.or(`name.ilike.%${filter.search.replace(/[%,()]/g, "")}%,description.ilike.%${filter.search.replace(/[%,()]/g, "")}%`);
      return page<Agent>(query.order(filter.sort ?? "volume", { ascending: false }).order("address"), index);
    },
    placeholderData: keepPreviousData,
  });

export const useCapabilities = () =>
  useQuery({
    queryKey: ["capabilities"],
    queryFn: async () => {
      const rows = await run<Array<{ capabilities: string[] }>>(supabase.from("agents").select("capabilities").eq("deregistered", false).limit(1000));
      const counts = new Map<string, number>();
      for (const row of rows) for (const c of row.capabilities) counts.set(c, (counts.get(c) ?? 0) + 1);
      return [...counts.entries()].sort((a, b) => b[1] - a[1]);
    },
    staleTime: 60_000,
  });

export const useAgent = (address: string) =>
  useQuery({
    queryKey: ["agent", address],
    queryFn: async () => {
      const [agents, payments, worked, ordered] = await Promise.all([
        run<Agent[]>(supabase.from("agents").select(AGENT).eq("address", address).limit(1)),
        run<ChainEvent[]>(
          supabase.from("events").select("*").eq("pallet", "AgentRegistry").in("name", ["Paid", "Rated", "TaskRecorded"]).eq("data->>agent", address).order("block_number", { ascending: false }).limit(50),
        ),
        run<Task[]>(supabase.from("tasks").select(TASK).eq("worker", address).order("id", { ascending: false }).limit(50)),
        run<Task[]>(supabase.from("tasks").select(TASK).eq("client", address).order("id", { ascending: false }).limit(50)),
      ]);
      return { agent: agents[0] ?? null, payments, worked, ordered };
    },
  });

const OPEN_TASKS: TaskStatus[] = ["open", "assigned", "delivered", "disputed"];

export const useTasks = (index: number, filter: { status?: "active" | "done" | TaskStatus; capability?: string; account?: string } = {}) =>
  useQuery({
    queryKey: ["tasks", index, filter],
    queryFn: () => {
      let query = supabase.from("tasks").select(TASK);
      if (filter.status === "active") query = query.in("status", OPEN_TASKS);
      else if (filter.status === "done") query = query.in("status", ["completed", "cancelled", "expired"]);
      else if (filter.status) query = query.eq("status", filter.status);
      if (filter.capability) query = query.eq("capability", filter.capability);
      if (filter.account) query = query.or(`client.eq.${filter.account},worker.eq.${filter.account},arbiter.eq.${filter.account}`);
      return page<Task>(query.order("id", { ascending: false }), index);
    },
    placeholderData: keepPreviousData,
  });

export const useTask = (id: number) =>
  useQuery({
    queryKey: ["task", id],
    queryFn: async () => {
      const [tasks, events] = await Promise.all([
        run<Task[]>(supabase.from("tasks").select(TASK).eq("id", id).limit(1)),
        run<ChainEvent[]>(supabase.from("events").select("*").eq("pallet", "Tasks").eq("data->>id", String(id)).order("block_number").order("index")),
      ]);
      return { task: tasks[0] ?? null, events };
    },
  });

export const useChannels = (index: number, filter: { status?: "open" | "closing" | "closed"; account?: string } = {}) =>
  useQuery({
    queryKey: ["channels", index, filter],
    queryFn: () => {
      let query = supabase.from("channels").select(CHANNEL);
      if (filter.status) query = query.eq("status", filter.status);
      if (filter.account) query = query.or(`payer.eq.${filter.account},payee.eq.${filter.account}`);
      return page<Channel>(query.order("id", { ascending: false }), index);
    },
    placeholderData: keepPreviousData,
  });

export const useChannel = (id: number) =>
  useQuery({
    queryKey: ["channel", id],
    queryFn: async () => {
      const [channels, events] = await Promise.all([
        run<Channel[]>(supabase.from("channels").select(CHANNEL).eq("id", id).limit(1)),
        run<ChainEvent[]>(supabase.from("events").select("*").eq("pallet", "Channels").eq("data->>id", String(id)).order("block_number").order("index")),
      ]);
      return { channel: channels[0] ?? null, events };
    },
  });

export const useShieldedActivity = (index: number) =>
  useQuery({
    queryKey: ["shielded", index],
    queryFn: () => page<ChainEvent>(supabase.from("events").select("*").eq("pallet", "Shielded").eq("name", "Transacted").order("block_number", { ascending: false }).order("index", { ascending: false }), index),
    placeholderData: keepPreviousData,
  });
