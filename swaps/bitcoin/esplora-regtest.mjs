#!/usr/bin/env node
// An Esplora API over a Bitcoin Core node in regtest, for testing swaps in BTC on one's own
// machine: the command line wallet, the SDKs and the web wallet read and broadcast through it as
// through mempool.space. Only the endpoints swaps use are served.
//
//   bitcoind -regtest -txindex=1 -fallbackfee=0.0001 -server -datadir=<dir>
//   node swaps/bitcoin/esplora-regtest.mjs --datadir <dir> [--port 3002] [--rpc http://127.0.0.1:18443]
//
// The node is reached with its cookie (`<datadir>/regtest/.cookie`).

import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => (arg.startsWith("--") ? [...pairs, [arg.slice(2), all[i + 1]]] : pairs), []),
);
const datadir = args.datadir ?? process.env.BITCOIN_DATADIR;
if (!datadir) {
  console.error("usage: esplora-regtest.mjs --datadir <bitcoind datadir> [--port 3002] [--rpc http://127.0.0.1:18443]");
  process.exit(1);
}
const rpcUrl = args.rpc ?? "http://127.0.0.1:18443";
const port = Number(args.port ?? 3002);

async function rpc(method, params = []) {
  const cookie = readFileSync(join(datadir, "regtest", ".cookie"), "utf8").trim();
  const response = await fetch(rpcUrl, {
    method: "POST",
    headers: { authorization: `Basic ${Buffer.from(cookie).toString("base64")}`, "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "1.0", id: 1, method, params }),
  });
  const body = await response.json();
  if (body.error) throw Object.assign(new Error(body.error.message), { code: body.error.code });
  return body.result;
}

const sats = (btc) => Math.round(btc * 1e8);

// Transactions by address, from the blocks read so far: address -> [{ txid, height, time }].
const history = new Map();
let indexed = 0;

function remember(address, entry) {
  if (!address) return;
  const list = history.get(address) ?? [];
  if (!list.some((e) => e.txid === entry.txid)) list.push(entry);
  history.set(address, list);
}

async function index() {
  const height = await rpc("getblockcount");
  for (; indexed <= height; indexed++) {
    const block = await rpc("getblock", [await rpc("getblockhash", [indexed]), 3]);
    for (const tx of block.tx) {
      const entry = { txid: tx.txid, height: indexed, time: block.time };
      for (const output of tx.vout) remember(output.scriptPubKey.address, entry);
      for (const input of tx.vin) remember(input.prevout?.scriptPubKey?.address, entry);
    }
  }
}

async function mempoolOf(address) {
  const found = [];
  for (const txid of await rpc("getrawmempool")) {
    const tx = await rpc("getrawtransaction", [txid, 2]);
    const outputs = tx.vout.some((o) => o.scriptPubKey.address === address);
    const inputs = tx.vin.some((i) => i.prevout?.scriptPubKey?.address === address);
    if (outputs || inputs) found.push(txid);
  }
  return found;
}

const status = (height, time) => (height === undefined ? { confirmed: false } : { confirmed: true, block_height: height, block_time: time });

const routes = [
  ["GET", /^\/blocks\/tip\/height$/, async () => String(await rpc("getblockcount"))],
  ["GET", /^\/blocks\/tip\/hash$/, async () => await rpc("getbestblockhash")],
  [
    "GET",
    /^\/block\/([0-9a-f]{64})$/,
    async ([hash]) => {
      const header = await rpc("getblockheader", [hash]);
      return { id: hash, height: header.height, timestamp: header.time, mediantime: header.mediantime };
    },
  ],
  ["GET", /^\/fee-estimates$/, async () => ({ 1: 2, 2: 2, 3: 1.5, 6: 1, 144: 1 })],
  [
    "GET",
    /^\/address\/([a-z0-9]+)\/utxo$/,
    async ([address]) => {
      const scan = await rpc("scantxoutset", ["start", [`addr(${address})`]]);
      const confirmed = scan.unspents.map((u) => ({ txid: u.txid, vout: u.vout, value: sats(u.amount), status: { confirmed: true, block_height: u.height } }));
      // Outputs of the mempool to the address, unspent.
      const pending = [];
      for (const txid of await mempoolOf(address)) {
        const tx = await rpc("getrawtransaction", [txid, 2]);
        for (const output of tx.vout) {
          if (output.scriptPubKey.address === address && (await rpc("gettxout", [txid, output.n, true]))) {
            pending.push({ txid, vout: output.n, value: sats(output.value), status: { confirmed: false } });
          }
        }
      }
      // Outputs spent by the mempool are no longer available.
      const spent = new Set();
      for (const u of confirmed) if (!(await rpc("gettxout", [u.txid, u.vout, true]))) spent.add(`${u.txid}:${u.vout}`);
      return [...pending, ...confirmed.filter((u) => !spent.has(`${u.txid}:${u.vout}`))];
    },
  ],
  [
    "GET",
    /^\/address\/([a-z0-9]+)\/txs(?:\/chain\/([0-9a-f]{64}))?$/,
    async ([address, after]) => {
      await index();
      const confirmed = [...(history.get(address) ?? [])].sort((a, b) => b.height - a.height);
      let start = 0;
      if (after) start = confirmed.findIndex((e) => e.txid === after) + 1;
      const page = confirmed.slice(start, start + 25).map((e) => ({ txid: e.txid, status: status(e.height, e.time) }));
      const pending = after ? [] : (await mempoolOf(address)).map((txid) => ({ txid, status: status() }));
      return [...pending, ...page];
    },
  ],
  ["GET", /^\/tx\/([0-9a-f]{64})\/hex$/, async ([txid]) => await rpc("getrawtransaction", [txid, 0])],
  ["GET", /^\/tx\/([0-9a-f]{64})\/raw$/, async ([txid]) => Buffer.from(await rpc("getrawtransaction", [txid, 0]), "hex")],
  [
    "GET",
    /^\/tx\/([0-9a-f]{64})\/status$/,
    async ([txid]) => {
      const tx = await rpc("getrawtransaction", [txid, 1]);
      if (!tx.blockhash) return status();
      const header = await rpc("getblockheader", [tx.blockhash]);
      return status(header.height, header.time);
    },
  ],
  [
    "GET",
    /^\/tx\/([0-9a-f]{64})\/outspend\/(\d+)$/,
    async ([txid, vout]) => {
      await rpc("getrawtransaction", [txid, 0]);
      return { spent: !(await rpc("gettxout", [txid, Number(vout), true])) };
    },
  ],
  ["POST", /^\/tx$/, async (_, body) => await rpc("sendrawtransaction", [body.trim()])],
];

createServer(async (request, response) => {
  const headers = { "access-control-allow-origin": "*", "access-control-allow-headers": "*" };
  if (request.method === "OPTIONS") return response.writeHead(204, headers).end();
  const path = new URL(request.url, "http://localhost").pathname;
  const route = routes.find(([method, pattern]) => method === request.method && pattern.test(path));
  if (!route) return response.writeHead(404, headers).end("not found");
  let body = "";
  for await (const chunk of request) body += chunk;
  try {
    const result = await route[2](path.match(route[1]).slice(1), body);
    if (Buffer.isBuffer(result)) return response.writeHead(200, { ...headers, "content-type": "application/octet-stream" }).end(result);
    if (typeof result === "string") return response.writeHead(200, { ...headers, "content-type": "text/plain" }).end(result);
    response.writeHead(200, { ...headers, "content-type": "application/json" }).end(JSON.stringify(result));
  } catch (error) {
    const notFound = error.code === -5;
    response.writeHead(notFound ? 404 : 400, headers).end(notFound ? "Transaction not found" : error.message);
  }
}).listen(port, "127.0.0.1", () => console.log(`Esplora of regtest on http://127.0.0.1:${port}`));
