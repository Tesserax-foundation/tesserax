/** Follows the best chain of a node into the explorer database. */

import { powDigest, type Runtime, Runtimes, u256 } from "./chain.js";
import * as db from "./db.js";
import { readStorage, refresh, storageKeys, Touched } from "./entities.js";
import { log } from "./log.js";
import type { Rpc } from "./rpc.js";

interface RpcHeader {
  parentHash: string;
  number: string;
  stateRoot: string;
  extrinsicsRoot: string;
  digest: { logs: string[] };
}

interface Fetched {
  hash: string;
  header: RpcHeader;
  extrinsics: string[];
  runtime: Runtime;
  events: string | null;
  difficulty: string | null;
}

export interface IndexerOptions {
  /** Blocks fetched in parallel while catching up. */
  batch: number;
  /** Interval between two reads of the genesis allocations, in blocks. */
  allocationsInterval: number;
}

const ZERO_HASH = "0x" + "00".repeat(32);

export class Indexer {
  private readonly runtimes: Runtimes;
  private constantsSpec = -1;
  private allocationsBlock = -Infinity;
  private wake?: () => void;
  private stopped = false;

  constructor(
    private readonly rpc: Rpc,
    private readonly sql: db.Sql,
    private readonly options: IndexerOptions = { batch: 16, allocationsInterval: 600 },
  ) {
    this.runtimes = new Runtimes(rpc);
  }

  /** Index until `stop` is called: on every new head, and every 10 s in any case. */
  async run(): Promise<void> {
    const subscribe = () =>
      this.rpc.subscribe("chain_subscribeNewHeads", [], () => this.wake?.()).catch((e) => log.warn(`cannot subscribe to new heads: ${e.message}`));
    this.rpc.onConnect = () => void subscribe();
    await this.rpc.connect();
    await subscribe();
    while (!this.stopped) {
      try {
        await this.sync();
      } catch (error) {
        const message = (error as Error).message ?? String(error);
        if (message.includes("State already discarded")) {
          log.error("the node discarded the state of blocks to index: run it as an archive node (--state-pruning archive)");
        } else {
          log.error(`sync failed: ${(error as Error).stack ?? error}`);
        }
        await new Promise((r) => setTimeout(r, 5_000));
      }
      await new Promise<void>((resolve) => {
        const timer = setTimeout(resolve, 10_000);
        this.wake = () => {
          clearTimeout(timer);
          resolve();
        };
      });
    }
  }

  stop(): void {
    this.stopped = true;
    this.wake?.();
  }

  /** Bring the database to the best block of the node. Returns the number of blocks indexed. */
  async sync(): Promise<number> {
    const best = await this.rpc.request<RpcHeader>("chain_getHeader");
    const bestNumber = parseInt(best.number, 16);
    const touched = new Touched();

    let tip = await db.canonicalTip(this.sql);
    if (!tip) tip = await this.indexGenesis();

    // Blocks of the database no longer in the best chain.
    const fork = await this.forkPoint(tip.number, bestNumber);
    if (fork < tip.number) {
      const orphaned = await db.orphanAbove(this.sql, fork);
      orphaned.events.forEach((e) => touched.event(e));
      log.warn(`reorganization: ${orphaned.hashes.length} block(s) above #${fork} left the best chain`);
    }

    let indexed = 0;
    let parent = fork === tip.number ? tip.hash : (await db.canonicalHash(this.sql, fork))!;
    let last: { hash: string; number: number; runtime: Runtime } | undefined;
    for (let start = fork + 1; start <= bestNumber; start += this.options.batch) {
      const numbers = Array.from({ length: Math.min(this.options.batch, bestNumber - start + 1) }, (_, i) => start + i);
      let started = performance.now();
      const fetched = await Promise.all(numbers.map((n) => this.fetch(n)));
      log.debug(`fetched ${numbers.length} blocks in ${Math.round(performance.now() - started)} ms`);
      started = performance.now();
      const rows: db.BlockRow[] = [];
      let changed = false;
      for (const block of fetched) {
        if (!block || block.header.parentHash !== parent) {
          // The best chain changed while fetching: the next sync starts from here.
          changed = true;
          break;
        }
        const row = this.decode(block);
        rows.push(row);
        row.extrinsics.forEach((x) => touched.account(x.signer));
        touched.account(row.author);
        row.events.forEach((e) => touched.event(e));
        parent = block.hash;
        last = { hash: block.hash, number: row.number, runtime: block.runtime };
      }
      log.debug(`decoded ${rows.length} blocks in ${Math.round(performance.now() - started)} ms`);
      started = performance.now();
      // The blocks of a batch go to the database in one transaction.
      await db.saveBlocks(this.sql, rows);
      log.debug(`saved ${rows.length} blocks in ${Math.round(performance.now() - started)} ms`);
      indexed += rows.length;
      if (changed) {
        await this.finish(touched, last);
        return indexed;
      }
      if (last) log.info(`indexed up to #${last.number} of #${bestNumber}`);
      // Entities are refreshed every batch while catching up.
      started = performance.now();
      const entities = touched.size;
      await this.finish(touched, last);
      log.debug(`refreshed ${entities} entities in ${Math.round(performance.now() - started)} ms`);
    }
    await this.finish(touched, last);

    const finalizedHash = await this.rpc.request<string>("chain_getFinalizedHead");
    const finalized = await this.rpc.request<RpcHeader>("chain_getHeader", [finalizedHash]);
    await db.markFinalized(this.sql, parseInt(finalized.number, 16));
    await this.updateState(best, parseInt(finalized.number, 16));
    return indexed;
  }

  /** Refresh the touched entities at `last`, and forget them. */
  private async finish(touched: Touched, last: { hash: string; number: number; runtime: Runtime } | undefined): Promise<void> {
    if (touched.size === 0) return;
    const at = last ?? (await this.head());
    await refresh(this.rpc, this.sql, at.runtime, touched, at.hash, at.number);
    touched.accounts.clear();
    touched.agents.clear();
    touched.delegations.clear();
    touched.channels.clear();
    touched.tasks.clear();
    touched.swaps = false;
  }

  private async head(): Promise<{ hash: string; number: number; runtime: Runtime }> {
    const tip = (await db.canonicalTip(this.sql))!;
    return { ...tip, runtime: await this.runtimes.at(tip.hash) };
  }

  /** The highest block of the database that is still in the best chain. */
  private async forkPoint(tip: number, best: number): Promise<number> {
    for (let n = tip; n > 0; n--) {
      const ours = await db.canonicalHash(this.sql, n);
      const theirs = n <= best ? await this.rpc.request<string | null>("chain_getBlockHash", [n]) : null;
      if (ours && ours === theirs) return n;
    }
    return 0;
  }

  private async fetch(number: number): Promise<Fetched | null> {
    const hash = await this.rpc.request<string | null>("chain_getBlockHash", [number]);
    if (!hash) return null;
    const { block } = await this.rpc.request<{ block: { header: RpcHeader; extrinsics: string[] } }>("chain_getBlock", [hash]);
    // Extrinsics and events of a block are encoded by the runtime of its parent.
    const runtime = await this.runtimes.at(block.header.parentHash);
    const [events, difficulty] = await Promise.all([
      this.rpc.request<string | null>("state_getStorage", [runtime.eventsKey, hash]),
      // The difficulty a block was mined at is the one of its parent state.
      this.rpc.request<string | null>("state_getStorage", [runtime.storageKey("Difficulty", "CurrentDifficulty"), block.header.parentHash]),
    ]);
    return { hash, header: block.header, extrinsics: block.extrinsics, runtime, events, difficulty };
  }

  private decode(block: Fetched): db.BlockRow {
    const { runtime, header } = block;
    const number = parseInt(header.number, 16);
    const digest = powDigest(header.digest.logs, runtime);
    const extrinsics = block.extrinsics.map((hex, index) => runtime.decodeExtrinsic(hex, index));
    const events = runtime.decodeEvents(block.events);

    const now = extrinsics.find((x) => x.pallet === "Timestamp" && x.call === "set")?.rawArgs?.now;
    const outcome = new Map<number, { success: boolean; error: string | null; fee: bigint | null }>();
    let reward = 0n;
    let fees = 0n;
    for (const e of events) {
      const index = e.extrinsicIndex;
      if (index != null && e.pallet === "System" && e.name === "ExtrinsicSuccess") {
        outcome.set(index, { ...(outcome.get(index) ?? { fee: null }), success: true, error: null });
      } else if (index != null && e.pallet === "System" && e.name === "ExtrinsicFailed") {
        outcome.set(index, { ...(outcome.get(index) ?? { fee: null }), success: false, error: runtime.errorName(e.data.dispatch_error) });
      } else if (index != null && e.pallet === "TransactionPayment" && e.name === "TransactionFeePaid") {
        const fee = BigInt(e.data.actual_fee);
        outcome.set(index, { ...(outcome.get(index) ?? { success: true, error: null }), fee });
        fees += fee;
      } else if (e.pallet === "Emission" && e.name === "Rewarded") {
        reward += BigInt(e.data.amount);
      }
    }

    const transfers: db.TransferRow[] = events
      .filter((e) => e.pallet === "Balances" && e.name === "Transfer")
      .map((e) => ({
        eventIndex: e.index,
        extrinsicIndex: e.extrinsicIndex,
        extrinsicHash: e.extrinsicIndex == null ? null : (extrinsics[e.extrinsicIndex]?.hash ?? null),
        from: e.data.from,
        to: e.data.to,
        amount: BigInt(e.data.amount),
      }));

    return {
      hash: block.hash,
      number,
      parentHash: header.parentHash,
      stateRoot: header.stateRoot,
      extrinsicsRoot: header.extrinsicsRoot,
      author: digest.author,
      timestamp: now == null ? null : new Date(Number(now)),
      difficulty: block.difficulty == null ? null : u256(runtime.decodeStorage("Difficulty", "CurrentDifficulty", block.difficulty)),
      powNonce: digest.nonce,
      powHash: digest.powHash,
      reward,
      fees,
      size: extrinsics.reduce((sum, x) => sum + x.size, 0),
      specVersion: runtime.specVersion,
      extrinsics: extrinsics.map((x) => ({
        index: x.index,
        hash: x.hash,
        signer: x.signer,
        pallet: x.pallet,
        call: x.call,
        args: x.args,
        nonce: x.nonce,
        tip: x.tip,
        fee: outcome.get(x.index)?.fee ?? null,
        success: outcome.get(x.index)?.success ?? true,
        error: outcome.get(x.index)?.error ?? null,
        size: x.size,
      })),
      events: events.map((e) => ({ index: e.index, extrinsicIndex: e.extrinsicIndex, pallet: e.pallet, name: e.name, data: e.data })),
      transfers,
      shielded: events.flatMap((e) => (e.shielded ? [{ ...e.shielded, eventIndex: e.index }] : [])),
    };
  }

  /** Index the genesis block and the accounts it endows. */
  private async indexGenesis(): Promise<{ number: number; hash: string }> {
    const hash = await this.rpc.request<string>("chain_getBlockHash", [0]);
    const { block } = await this.rpc.request<{ block: { header: RpcHeader } }>("chain_getBlock", [hash]);
    const runtime = await this.runtimes.at(hash);
    await db.saveBlock(this.sql, {
      hash,
      number: 0,
      parentHash: block.header.parentHash ?? ZERO_HASH,
      stateRoot: block.header.stateRoot,
      extrinsicsRoot: block.header.extrinsicsRoot,
      author: null,
      timestamp: null,
      difficulty: null,
      powNonce: null,
      powHash: null,
      reward: 0n,
      fees: 0n,
      size: 0,
      specVersion: runtime.specVersion,
      extrinsics: [],
      events: [],
      transfers: [],
      shielded: [],
    });
    const prefix = runtime.storageKey("System", "Account");
    const storage = runtime.builder.buildStorage("System", "Account");
    const touched = new Touched();
    for (const key of await storageKeys(this.rpc, prefix, hash)) touched.account(storage.keys.dec(key)[0]);
    await refresh(this.rpc, this.sql, runtime, touched, hash, 0);
    log.info(`indexed the genesis block and ${touched.accounts.size} genesis accounts`);
    return { number: 0, hash };
  }

  /** Head, supply, shielded pool, constants and allocations in `chain_state`. */
  private async updateState(best: RpcHeader, finalized: number): Promise<void> {
    const tip = await db.canonicalTip(this.sql);
    if (!tip) return;
    const runtime = await this.runtimes.at(tip.hash);
    const keys = {
      issuance: runtime.storageKey("Balances", "TotalIssuance"),
      minted: runtime.storageKey("Emission", "TotalMinted"),
      pool: runtime.storageKey("Shielded", "PoolBalance"),
      tree: runtime.storageKey("Shielded", "Tree"),
      difficulty: runtime.storageKey("Difficulty", "CurrentDifficulty"),
    };
    const values = await readStorage(this.rpc, Object.values(keys), tip.hash);
    const read = (pallet: string, item: string, key: string) => runtime.decodeStorage(pallet, item, values.get(key) ?? null);

    const initialReward = BigInt(runtime.constantValue("Emission", "InitialReward"));
    const halving = Number(runtime.constantValue("Emission", "HalvingInterval"));
    const next = tip.number + 1;
    const era = Math.floor((next - 1) / halving);
    await db.setState(this.sql, "head", {
      best: tip.number,
      best_hash: tip.hash,
      node_best: parseInt(best.number, 16),
      finalized,
      spec_version: runtime.specVersion,
      next_difficulty: u256(read("Difficulty", "CurrentDifficulty", keys.difficulty)).toString(),
    });
    await db.setState(this.sql, "supply", {
      total_issuance: BigInt(read("Balances", "TotalIssuance", keys.issuance)).toString(),
      mined: BigInt(read("Emission", "TotalMinted", keys.minted)).toString(),
      mining_supply: BigInt(runtime.constantValue("Emission", "MiningSupply")).toString(),
      block_reward: (era >= 128 ? 0n : initialReward >> BigInt(era)).toString(),
      halving_era: era,
      next_halving: (era + 1) * halving + 1,
      shielded_pool: BigInt(read("Shielded", "PoolBalance", keys.pool)).toString(),
      shielded_notes: Number(read("Shielded", "Tree", keys.tree)?.size ?? 0),
    });

    if (this.constantsSpec !== runtime.specVersion) {
      await db.setState(this.sql, "constants", {
        token: "TESS",
        decimals: 12,
        ss58_prefix: runtime.ss58,
        spec_name: "tesserax",
        spec_version: runtime.specVersion,
        target_block_time_ms: Number(runtime.constantValue("Difficulty", "TargetBlockTime")),
        difficulty_window: Number(runtime.constantValue("Difficulty", "Window")),
        initial_reward: initialReward.toString(),
        halving_interval: halving,
        mining_supply: BigInt(runtime.constantValue("Emission", "MiningSupply")).toString(),
        existential_deposit: BigInt(runtime.constantValue("Balances", "ExistentialDeposit")).toString(),
        channel_close_period: Number(runtime.constantValue("Channels", "ClosePeriod")),
        task_arbitration_period: Number(runtime.constantValue("Tasks", "ArbitrationPeriod")),
        shielded_minimum_fee: BigInt(runtime.constantValue("Shielded", "MinimumFee")).toString(),
      });
      this.constantsSpec = runtime.specVersion;
    }

    if (tip.number - this.allocationsBlock >= this.options.allocationsInterval) {
      await this.updateAllocations(runtime, tip.hash);
      this.allocationsBlock = tip.number;
    }
  }

  /** Genesis allocations (team and foundation) and what they still have locked in vesting. */
  private async updateAllocations(runtime: Runtime, at: string): Promise<void> {
    const storage = runtime.builder.buildStorage("Allocations", "Allocations");
    const keys = await storageKeys(this.rpc, runtime.storageKey("Allocations", "Allocations"), at);
    const accounts = keys.map((k) => storage.keys.dec(k)[0] as string);
    const vestingKeys = accounts.map((a) => runtime.storageKey("Vesting", "Vesting", a));
    const values = await readStorage(this.rpc, [...keys, ...vestingKeys], at);
    const allocations = accounts.map((account, i) => {
      const allocation = runtime.decodeStorage("Allocations", "Allocations", values.get(keys[i]) ?? null);
      const schedules = (runtime.decodeStorage("Vesting", "Vesting", values.get(vestingKeys[i]) ?? null) ?? []) as any[];
      return {
        account,
        amount: BigInt(allocation.amount).toString(),
        liquid: BigInt(allocation.liquid).toString(),
        cliff: Number(allocation.cliff),
        duration: Number(allocation.duration),
        vesting: schedules.map((s) => ({
          locked: BigInt(s.locked).toString(),
          per_block: BigInt(s.per_block).toString(),
          starting_block: Number(s.starting_block),
        })),
      };
    });
    await db.setState(this.sql, "allocations", allocations);
  }
}
