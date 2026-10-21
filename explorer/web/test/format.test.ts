import assert from "node:assert/strict";
import { test } from "node:test";
import { formatCompact, formatDuration, formatTess, hexText, humanize, isAddress, isBlockNumber, isHash, plural, shortHash, timeAgo } from "../src/lib/format";

test("amounts in TESS keep their precision", () => {
  assert.equal(formatTess("850000000000"), "0.85");
  assert.equal(formatTess("14000000000000000000"), "14,000,000");
  assert.equal(formatTess("1000229637900000000", 4), "1,000,229.6379");
  // Small amounts keep their significant digits.
  assert.equal(formatTess("2055111304"), "0.002055");
  assert.equal(formatTess("2055111304", 12), "0.002055111304");
  assert.equal(formatTess("0"), "0");
  assert.equal(formatTess(null), "—");
  // Beyond 2^64 planck.
  assert.equal(formatTess((2n ** 70n).toString(), 0), "1,180,591,620");
});

test("numbers, durations and plurals", () => {
  assert.equal(formatCompact(4420), "4.42 K");
  assert.equal(formatCompact("123456789", "H/s"), "123 MH/s");
  assert.equal(formatDuration(18), "18 s");
  assert.equal(formatDuration(126000), "35 h");
  assert.equal(formatDuration(7_000_000 * 18), "4.0 years");
  assert.equal(plural(1, "miner"), "1 miner");
  assert.equal(plural(2, "miner"), "2 miners");
  assert.equal(plural(1200, "tx", "txs"), "1,200 txs");
  assert.equal(timeAgo(new Date(Date.now() - 90_000)), "1 min ago");
});

test("search inputs", () => {
  assert.ok(isAddress("teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5"));
  assert.ok(!isAddress("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY"));
  assert.ok(isHash("0x" + "ab".repeat(32)));
  assert.ok(isBlockNumber("#106") && isBlockNumber("106") && !isBlockNumber("10a"));
  assert.equal(shortHash("0x" + "ab".repeat(32)), "0xababab…abab");
});

test("byte strings shown as text when they are text", () => {
  assert.equal(hexText("0x7472616e736c61746f72"), "translator");
  assert.equal(hexText("0x00ff10"), null);
  assert.equal(humanize("transfer_keep_alive"), "Transfer keep alive");
  assert.equal(humanize("BudgetExceeded"), "Budget exceeded");
});
