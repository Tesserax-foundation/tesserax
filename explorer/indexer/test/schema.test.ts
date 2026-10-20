import assert from "node:assert/strict";
import { test } from "node:test";
import * as db from "../src/db.js";
import { testDatabase } from "./database.js";

const block = (number: number, hash: string, parentHash: string, extra: Partial<db.BlockRow> = {}): db.BlockRow => ({
  hash,
  number,
  parentHash,
  stateRoot: "0x01",
  extrinsicsRoot: "0x02",
  author: "teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5",
  timestamp: new Date(),
  difficulty: 2n ** 70n,
  powNonce: "0x00",
  powHash: "0x00",
  reward: 850_000_000_000n,
  fees: 0n,
  size: 11,
  specVersion: 100,
  extrinsics: [],
  events: [],
  transfers: [],
  ...extra,
});

test("blocks, reorganizations and the read functions", async () => {
  const database = await testDatabase();
  const sql = db.connect(database.url);
  try {
    // Amounts beyond 64 bits keep their precision.
    const big = 14_000_000n * 10n ** 12n;
    await db.saveBlock(sql, block(0, "0xa0", "0x00"));
    await db.saveBlock(
      sql,
      block(1, "0xa1", "0xa0", {
        extrinsics: [{ index: 0, hash: "0xe1", signer: "teAlice", pallet: "Balances", call: "transfer_keep_alive",
          args: { dest: "teBob", value: big.toString(), nested: [1, { a: null }] }, nonce: 0, tip: 0n, fee: 10n,
          success: true, error: null, size: 7300 }],
        events: [{ index: 0, extrinsicIndex: 0, pallet: "Balances", name: "Transfer", data: { from: "teAlice", to: "teBob", amount: big.toString() } }],
        transfers: [{ eventIndex: 0, extrinsicIndex: 0, extrinsicHash: "0xe1", from: "teAlice", to: "teBob", amount: big }],
      }),
    );
    await db.saveBlock(sql, block(2, "0xa2", "0xa1"));
    assert.deepEqual(await db.canonicalTip(sql), { number: 2, hash: "0xa2" });

    const [transfer] = await sql`select amount::text, from_address from transfers`;
    assert.equal(transfer.amount, big.toString());
    const [x] = await sql`select args from extrinsics where hash = '0xe1'`;
    assert.deepEqual(x.args.nested, [1, { a: null }]);

    // A reorganization replaces blocks 1 and 2.
    const orphaned = await db.orphanAbove(sql, 0);
    assert.deepEqual(orphaned.hashes.sort(), ["0xa1", "0xa2"]);
    assert.equal(orphaned.events.length, 1);
    assert.equal((await sql`select count(*)::int as n from extrinsics`)[0].n, 0);
    await db.saveBlock(sql, block(1, "0xb1", "0xa0"));
    assert.deepEqual(await db.canonicalTip(sql), { number: 1, hash: "0xb1" });
    const blocks = await sql`select hash, canonical from blocks order by hash`;
    assert.deepEqual(blocks.map((b) => [b.hash, b.canonical]), [["0xa0", true], ["0xa1", false], ["0xa2", false], ["0xb1", true]]);
    // Only one canonical block per height.
    await assert.rejects(sql`update blocks set canonical = true where hash = '0xa1'`);

    // Blocks orphaned before can come back.
    await db.orphanAbove(sql, 0);
    await db.saveBlock(sql, block(1, "0xa1", "0xa0"));
    assert.deepEqual(await db.canonicalTip(sql), { number: 1, hash: "0xa1" });

    await db.markFinalized(sql, 1);
    await db.setState(sql, "supply", { total_issuance: big.toString() });
    await db.upsertAccounts(sql, [{ address: "teBob", free: big, reserved: 0n, frozen: 0n, nonce: 0 }], 1);

    const [{ explorer_overview: overview }] = await sql`select explorer_overview()`;
    assert.equal(overview.best, 1);
    assert.equal(overview.finalized, 1);
    const [rich] = await sql`select * from rich_list(10, 0)`;
    assert.equal(rich.total, big.toString());
    assert.equal(Number(rich.share), 100);
    assert.deepEqual((await sql`select kind, id from explorer_search('1')`).map((r) => r.kind), ["block"]);
    assert.equal((await sql`select * from daily_stats(7)`).length, 1);
    assert.equal((await sql`select * from top_miners('7 days', 10)`)[0].blocks, "2");
  } finally {
    await sql.end();
    await database.close();
  }
});
