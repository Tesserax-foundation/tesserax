/**
 * The indexer against a running development chain:
 *
 *   tesserax --dev --tmp --state-pruning archive --mine --author <address>
 *   TESSERAX_URL=ws://127.0.0.1:9944 npm test
 */

import assert from "node:assert/strict";
import { test } from "node:test";
import { Runtimes } from "../src/chain.js";
import * as db from "../src/db.js";
import { readStorage, storageKeys } from "../src/entities.js";
import { Indexer } from "../src/indexer.js";
import { Rpc } from "../src/rpc.js";
import { testDatabase } from "./database.js";

const url = process.env.TESSERAX_URL;

test("indexes a development chain and follows a reorganization", { skip: !url && "set TESSERAX_URL to a development node", timeout: 900_000 }, async () => {
  const database = await testDatabase();
  const sql = db.connect(database.url);
  const rpc = new Rpc(url!);
  try {
    const indexer = new Indexer(rpc, sql, { batch: 8, allocationsInterval: 600 });
    await indexer.sync();
    const tip = (await db.canonicalTip(sql))!;
    assert.ok(tip.number > 0);

    // One canonical block per height, each the child of the previous one, as on the node.
    const blocks = await sql`select number, hash, parent_hash from blocks where canonical order by number`;
    assert.equal(blocks.length, tip.number + 1);
    for (let i = 1; i < blocks.length; i++) assert.equal(blocks[i].parent_hash, blocks[i - 1].hash);
    for (const n of [0, Math.floor(tip.number / 2), tip.number]) {
      assert.equal(blocks[n].hash, await rpc.request("chain_getBlockHash", [n]));
    }

    // Every mined block has its author, reward and difficulty.
    const [mined] = await sql`
      select count(*)::int as n, count(author)::int as authors, min(reward)::text as reward,
        min(difficulty)::text as difficulty from blocks where canonical and number > 0`;
    assert.equal(mined.authors, mined.n);
    assert.equal(mined.reward, "850000000000");
    assert.ok(BigInt(mined.difficulty) >= 1000n);

    // Signed extrinsics carry their signer, nonce and fee.
    const signed = await sql`select signer, nonce, fee::text, success, error from extrinsics where signer is not null`;
    assert.ok(signed.length > 0, "the chain has signed extrinsics");
    for (const x of signed) {
      assert.match(x.signer, /^te/);
      assert.ok(x.nonce !== null && BigInt(x.fee ?? "0") > 0n);
      assert.equal(x.success, x.error === null);
    }
    const [{ events, transfers }] = await sql`
      select (select count(*)::int from events where pallet = 'Balances' and name = 'Transfer') as events,
             (select count(*)::int from transfers) as transfers`;
    assert.equal(transfers, events);

    // Accounts as on chain.
    const runtime = await new Runtimes(rpc).at(tip.hash);
    const accounts = await sql`select address, free::text, nonce from accounts`;
    const values = await readStorage(rpc, accounts.map((a) => runtime.storageKey("System", "Account", a.address)), tip.hash);
    for (const a of accounts) {
      const info = runtime.decodeStorage("System", "Account", values.get(runtime.storageKey("System", "Account", a.address)) ?? null);
      assert.equal(a.free, BigInt(info.data.free).toString(), a.address);
      assert.equal(Number(a.nonce), Number(info.nonce), a.address);
    }

    // Registered agents, open channels and open tasks as on chain.
    const onChain = async (pallet: string, item: string) => (await storageKeys(rpc, runtime.storageKey(pallet, item), tip.hash)).length;
    assert.equal((await sql`select count(*)::int as n from agents where not deregistered`)[0].n, await onChain("AgentRegistry", "Agents"));
    assert.equal((await sql`select count(*)::int as n from channels where status <> 'closed'`)[0].n, await onChain("Channels", "Channels"));
    assert.equal(
      (await sql`select count(*)::int as n from tasks where status in ('open', 'assigned', 'delivered', 'disputed')`)[0].n,
      await onChain("Tasks", "Tasks"),
    );

    // Open offers and bids as on chain; bids since runtime 106.
    assert.equal((await sql`select count(*)::int as n from swap_offers`)[0].n, await onChain("Swaps", "Offers"));
    if (runtime.hasStorage("Swaps", "Bids")) {
      assert.equal((await sql`select count(*)::int as n from swap_bids`)[0].n, await onChain("Swaps", "Bids"));
    }

    // Tasks and channels created then finished between two refreshes are indexed too.
    const [created] = await sql`
      select (select count(distinct data->>'id')::int from events where pallet = 'Tasks' and name = 'Created') as tasks,
             (select count(*)::int from tasks) as task_rows,
             (select count(distinct data->>'id')::int from events where pallet = 'Channels' and name = 'Opened') as channels,
             (select count(*)::int from channels) as channel_rows`;
    assert.equal(created.task_rows, created.tasks);
    assert.equal(created.channel_rows, created.channels);
    const finished = await sql`select spec, reward::text, paid::text from tasks where status = 'completed'`;
    for (const t of finished) assert.ok(t.spec.length > 0 && BigInt(t.paid) > 0n, JSON.stringify(t));

    const state = Object.fromEntries((await sql`select key, value from chain_state`).map((r) => [r.key, r.value]));
    const issuance = runtime.decodeStorage("Balances", "TotalIssuance", (await readStorage(rpc, [runtime.storageKey("Balances", "TotalIssuance")], tip.hash)).values().next().value ?? null);
    assert.equal(state.supply.total_issuance, BigInt(issuance).toString());
    assert.equal(state.constants.ss58_prefix, 2261);
    assert.ok(Array.isArray(state.allocations));
    assert.equal(state.head.best, tip.number);

    // A reorganization: the database holds two blocks the node does not have.
    const top = (await db.canonicalTip(sql))!;
    await db.orphanAbove(sql, top.number - 2);
    const fake = (n: number): db.BlockRow => ({
      hash: `0xfake${n}`, number: n, parentHash: "0x", stateRoot: "0x", extrinsicsRoot: "0x", author: null,
      timestamp: new Date(), difficulty: 1n, powNonce: null, powHash: null, reward: 0n, fees: 0n, size: 0,
      specVersion: 100, extrinsics: [], events: [], transfers: [], shielded: [],
    });
    await db.saveBlock(sql, fake(top.number - 1));
    await db.saveBlock(sql, fake(top.number));
    await indexer.sync();
    const fakes = await sql`select canonical from blocks where hash like '0xfake%'`;
    assert.deepEqual(fakes.map((f) => f.canonical), [false, false]);
    for (const n of [top.number - 1, top.number]) {
      assert.equal(await db.canonicalHash(sql, n), await rpc.request("chain_getBlockHash", [n]));
    }
  } finally {
    rpc.close();
    await sql.end();
    await database.close();
  }
});
