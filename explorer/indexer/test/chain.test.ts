import assert from "node:assert/strict";
import { test } from "node:test";
import { Runtime } from "../src/chain.js";
import { swapBid, Touched } from "../src/entities.js";

test("dispatch error names", () => {
  // errorName does not read the metadata for decoded errors.
  const errorName = Runtime.prototype.errorName.bind({ metadata: { pallets: [] } } as any);
  assert.equal(errorName({ Module: { Agents: "BudgetExceeded" } }), "Agents.BudgetExceeded");
  assert.equal(errorName({ Module: { Tasks: { NoArbiter: null } } }), "Tasks.NoArbiter");
  assert.equal(errorName({ Token: "FundsUnavailable" }), "Token.FundsUnavailable");
  assert.equal(errorName("BadOrigin"), "BadOrigin");
});

test("storage items of a runtime", () => {
  const metadata = { pallets: [{ name: "Swaps", storage: { items: [{ name: "Offers" }, { name: "Bids" }] } }, { name: "Timestamp" }] };
  const hasStorage = Runtime.prototype.hasStorage.bind({ metadata } as any);
  assert.equal(hasStorage("Swaps", "Bids"), true);
  assert.equal(hasStorage("Swaps", "Sales"), false);
  assert.equal(hasStorage("Timestamp", "Now"), false);
  assert.equal(hasStorage("Tasks", "Tasks"), false);
});

test("bids decoded from their storage", () => {
  const bid = swapBid(7, {
    buyer: "teBuyer",
    payment: { chain: { type: "Bitcoin" }, asset: { type: "Native" } },
    price: 2_000n,
    wanted: 10n ** 30n,
    min_sale: 10n ** 12n,
    created: 42,
    sales: 3,
    completed: 1,
    deposit: 10n ** 13n,
  });
  assert.deepEqual(bid, {
    id: 7, buyer: "teBuyer", chain: "Bitcoin", asset: "Native", price: 2_000n, wanted: 10n ** 30n, minSale: 10n ** 12n,
    sales: 3, completed: 1, created: 42,
  });
});

test("a swap event reads the offers and bids again", () => {
  const touched = new Touched();
  touched.event({ pallet: "Swaps", name: "SaleClaimed", data: { sale: "0", buyer: "teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5", amount: "1", secret: "0x00" } });
  assert.equal(touched.swaps, true);
  assert.deepEqual([...touched.accounts], ["teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5"]);
  assert.equal(touched.size, 2);
});
