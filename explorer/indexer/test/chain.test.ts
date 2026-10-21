import assert from "node:assert/strict";
import { test } from "node:test";
import { Runtime } from "../src/chain.js";

test("dispatch error names", () => {
  // errorName does not read the metadata for decoded errors.
  const errorName = Runtime.prototype.errorName.bind({ metadata: { pallets: [] } } as any);
  assert.equal(errorName({ Module: { Agents: "BudgetExceeded" } }), "Agents.BudgetExceeded");
  assert.equal(errorName({ Module: { Tasks: { NoArbiter: null } } }), "Tasks.NoArbiter");
  assert.equal(errorName({ Token: "FundsUnavailable" }), "Token.FundsUnavailable");
  assert.equal(errorName("BadOrigin"), "BadOrigin");
});
