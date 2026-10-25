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
  shielded: [],
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

test("shielded transactions follow the canonical chain", async () => {
  const database = await testDatabase();
  const sql = db.connect(database.url);
  try {
    const note = "0x" + "ab".repeat(1232);
    const shielded = (position: bigint): db.ShieldedRow => ({
      eventIndex: 0,
      position,
      nullifiers: ["0x" + "01".repeat(32), "0x" + "02".repeat(32)],
      commitments: ["0x" + "03".repeat(32), "0x" + "04".repeat(32)],
      notes: [note, note],
      valueIn: 10n ** 13n,
      valueOut: 0n,
      fee: 0n,
    });
    await db.saveBlock(sql, block(0, "0xa0", "0x00"));
    await db.saveBlock(sql, block(1, "0xa1", "0xa0", { shielded: [shielded(0n)] }));
    await db.saveBlock(sql, block(2, "0xa2", "0xa1", { shielded: [shielded(2n)] }));
    const rows = await sql`select position, notes, nullifiers from shielded_transactions order by position`;
    assert.deepEqual(rows.map((r) => Number(r.position)), [0, 2]);
    // The encrypted notes are kept in full, unlike the event data.
    assert.equal(rows[0].notes[0], note);

    // A reorganization drops the transactions of the blocks that left the chain.
    await db.orphanAbove(sql, 1);
    assert.deepEqual((await sql`select position from shielded_transactions`).map((r) => Number(r.position)), [0]);
  } finally {
    await sql.end();
    await database.close();
  }
});


test("the market: pairs, tickers, order book, trades and supply", async () => {
  const database = await testDatabase();
  const sql = db.connect(database.url);
  const TESS = 10n ** 12n;
  const filled = (trade: number, amount: bigint, price: bigint, asset = "Usdc") => ({
    pallet: "Swaps",
    name: "Filled",
    data: { trade: String(trade), offer: "0", maker: "teMaker", taker: "teTaker", amount: amount.toString(), price: price.toString(), payment: { chain: "Base", asset }, hash: "0x00", payment_lock: "0x01", expires: "1300" },
  });
  try {
    await db.saveBlock(sql, block(0, "0xb0", "0x00"));
    await db.saveBlock(
      sql,
      block(1, "0xb1", "0xb0", {
        events: [
          { index: 0, extrinsicIndex: 0, ...filled(0, 100n * TESS, 50_000n) },
          { index: 1, extrinsicIndex: 1, ...filled(1, 3n * TESS + 1n, 60_000n) },
          { index: 2, extrinsicIndex: 2, ...filled(2, 5n * TESS, 10n ** 14n, "Native") },
        ],
      }),
    );
    await db.saveBlock(
      sql,
      block(2, "0xb2", "0xb1", {
        events: [
          { index: 0, extrinsicIndex: 0, pallet: "Swaps", name: "Claimed", data: { trade: "0", taker: "teTaker", amount: (100n * TESS).toString(), secret: "0x02" } },
          { index: 1, extrinsicIndex: 1, pallet: "Swaps", name: "Claimed", data: { trade: "1", taker: "teTaker", amount: (3n * TESS + 1n).toString(), secret: "0x03" } },
          { index: 2, extrinsicIndex: null, pallet: "Swaps", name: "Refunded", data: { trade: "2", maker: "teMaker", amount: (5n * TESS).toString() } },
        ],
      }),
    );
    await db.replaceSwapOffers(
      sql,
      [
        { id: 0, maker: "teMaker", chain: "Base", asset: "Usdc", price: 70_000n, available: 400n * TESS, minFill: TESS, payout: "0xaa", fills: 2, completed: 2 },
        { id: 1, maker: "teOther", chain: "Arbitrum", asset: "Usdc", price: 65_000n, available: 10n * TESS, minFill: TESS, payout: "0xbb", fills: 0, completed: 0 },
        { id: 2, maker: "teOther", chain: "Arbitrum", asset: "Usdc", price: 70_000n, available: 5n * TESS, minFill: TESS, payout: "0xbb", fills: 0, completed: 0 },
        { id: 3, maker: "teOther", chain: "Ethereum", asset: "Usdt", price: 80_000n, available: 0n, minFill: TESS, payout: "0xcc", fills: 1, completed: 1 },
      ],
      2,
    );

    const [{ pairs }] = await sql`select market_pairs() as pairs`;
    assert.deepEqual(pairs, [{ ticker_id: "TESS_USDC", base: "TESS", target: "USDC" }]);

    const [{ tickers }] = await sql`select market_tickers() as tickers`;
    assert.deepEqual(tickers, [
      {
        ticker_id: "TESS_USDC",
        base_currency: "TESS",
        target_currency: "USDC",
        last_price: "0.06",
        // 100 TESS at 0.05 and 3.000000000001 TESS at 0.06, the payment rounded up.
        base_volume: "103.000000000001",
        target_volume: "5.180001",
        bid: "0",
        ask: "0.065",
        high: "0.06",
        low: "0.05",
      },
    ]);

    const [{ book }] = await sql`select market_orderbook('TESS_USDC', 10) as book`;
    assert.deepEqual(book.asks, [["0.065", "10"], ["0.07", "405"]]);
    assert.deepEqual(book.bids, []);

    const [{ trades }] = await sql`select market_trades('TESS_USDC') as trades`;
    assert.deepEqual(trades.buy.map((t: { trade_id: number; price: string }) => [t.trade_id, t.price]), [[1, "0.06"], [0, "0.05"]]);
    const [{ none }] = await sql`select market_trades('TESS_USDC', 'sell') as none`;
    assert.deepEqual(none.buy, []);

    // 14 million TESS at most; 1,000 still vesting at block 100 of a schedule of 2,000 from 0.
    await db.setState(sql, "head", { best: 100 });
    await db.setState(sql, "supply", { total_issuance: (5_000_000n * TESS).toString(), mining_supply: (11_900_000n * TESS).toString() });
    await db.setState(sql, "allocations", [
      { account: "teTeam", amount: (2_100_000n * TESS).toString(), vesting: [{ locked: (2_000n * TESS).toString(), per_block: (10n * TESS).toString(), starting_block: 0 }] },
    ]);
    const [{ supply }] = await sql`select market_supply() as supply`;
    assert.deepEqual(supply, { total: "5000000", circulating: "4999000", max: "14000000" });
  } finally {
    await sql.end();
    await database.close();
  }
});
