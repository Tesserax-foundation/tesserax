import assert from "node:assert/strict";
import { test } from "node:test";
import { coingecko, deploymentRpc, respond, supply, type Rpc } from "../src/lib/marketApi";

/** A database answering every function with its name and arguments. */
const calls: Array<[string, Record<string, unknown> | undefined]> = [];
const rpc: Rpc = async (name, args) => {
  calls.push([name, args]);
  return name === "market_supply" ? { total: "5000000", circulating: "4999000", max: "14000000" } : { name, args };
};

test("CoinGecko endpoints call the market functions with checked parameters", async () => {
  assert.deepEqual((await coingecko("tickers", new URLSearchParams(), rpc)).body, { name: "market_tickers", args: undefined });
  const book = await coingecko("orderbook", new URLSearchParams("ticker_id=TESS_USDC&depth=0"), rpc);
  assert.deepEqual(book.body, { name: "market_orderbook", args: { ticker_id: "TESS_USDC", depth: 500 } });
  const trades = await coingecko("historical_trades", new URLSearchParams("ticker_id=TESS_ETH&type=buy&limit=10&start_time=1700000000"), rpc);
  assert.deepEqual(trades.body, {
    name: "market_trades",
    args: { ticker_id: "TESS_ETH", kind: "buy", max_rows: 10, start_time: "2023-11-14T22:13:20.000Z", end_time: null },
  });
  // Milliseconds are read as such.
  const ms = await coingecko("historical_trades", new URLSearchParams("ticker_id=TESS_ETH&end_time=1700000000000"), rpc);
  assert.equal((ms.body as { args: { end_time: string } }).args.end_time, "2023-11-14T22:13:20.000Z");

  const before = calls.length;
  assert.equal((await coingecko("orderbook", new URLSearchParams(), rpc)).status, 400);
  assert.equal((await coingecko("orderbook", new URLSearchParams("ticker_id=tess_usdc'--"), rpc)).status, 400);
  assert.equal((await coingecko("historical_trades", new URLSearchParams("ticker_id=TESS_USDC&type=both"), rpc)).status, 400);
  assert.equal((await coingecko("historical_trades", new URLSearchParams("ticker_id=TESS_USDC&limit=-1"), rpc)).status, 400);
  assert.equal((await coingecko("summary", new URLSearchParams(), rpc)).status, 404);
  assert.equal(calls.length, before, "a refused request reaches no function");
});

test("the supply as JSON or as one number", async () => {
  assert.deepEqual((await supply(undefined, rpc)).body, { total: "5000000", circulating: "4999000", max: "14000000" });
  const circulating = respond(await supply("circulating", rpc));
  assert.equal(await circulating.text(), "4999000");
  assert.match(circulating.headers.get("content-type")!, /text\/plain/);
  assert.equal(circulating.headers.get("access-control-allow-origin"), "*");
  assert.equal((await supply("burned", rpc)).status, 404);
});

test("a deployment without its database says so", () => {
  const answer = deploymentRpc({});
  assert.equal(typeof answer, "object");
  assert.equal((answer as { status: number }).status, 503);
  assert.equal(typeof deploymentRpc({ VITE_SUPABASE_URL: "https://x.supabase.co", VITE_SUPABASE_ANON_KEY: "k" }), "function");
});
