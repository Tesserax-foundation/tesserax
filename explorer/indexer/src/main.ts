/**
 * Tesserax Explorer indexer.
 *
 *   NODE_URL       websocket RPC of a Tesserax node (default ws://127.0.0.1:9944)
 *   DATABASE_URL   Postgres connection string of the Supabase project
 *   BATCH          blocks fetched in parallel while catching up (default 16)
 *   LOG_LEVEL      debug, info, warn or error (default info)
 */

import { connect } from "./db.js";
import { Indexer } from "./indexer.js";
import { log } from "./log.js";
import { Rpc } from "./rpc.js";

const nodeUrl = process.env.NODE_URL ?? "ws://127.0.0.1:9944";
const databaseUrl = process.env.DATABASE_URL;
if (!databaseUrl) {
  log.error("set DATABASE_URL to the Postgres connection string of the Supabase project");
  process.exit(1);
}

const rpc = new Rpc(nodeUrl);
const sql = connect(databaseUrl);
const indexer = new Indexer(rpc, sql, {
  batch: Math.max(1, Number(process.env.BATCH ?? 16)),
  allocationsInterval: 600,
});

const shutdown = async () => {
  log.info("stopping");
  indexer.stop();
  rpc.close();
  await sql.end({ timeout: 5 });
  process.exit(0);
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

log.info(`indexing ${nodeUrl}`);
await indexer.run();
