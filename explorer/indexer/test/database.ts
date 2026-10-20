/** A Postgres database for the tests: PGlite with the explorer migrations, served over the
 * Postgres protocol so that the indexer connects to it as to Supabase. */

import { PGlite } from "@electric-sql/pglite";
import { PGLiteSocketServer } from "@electric-sql/pglite-socket";
import { readdirSync, readFileSync } from "node:fs";

const MIGRATIONS = new URL("../../supabase/migrations/", import.meta.url);

/** What a Supabase project provides before the migrations run. */
const SUPABASE = `
  create role anon nologin;
  create role authenticated nologin;
  create publication supabase_realtime;
`;

export async function testDatabase(): Promise<{ url: string; db: PGlite; close: () => Promise<void> }> {
  const db = new PGlite();
  await db.exec(SUPABASE);
  for (const file of readdirSync(MIGRATIONS).filter((f) => f.endsWith(".sql")).sort()) {
    await db.exec(readFileSync(new URL(file, MIGRATIONS), "utf8"));
  }
  const port = 45000 + Math.floor(Math.random() * 10000);
  const server = new PGLiteSocketServer({ db, port, host: "127.0.0.1" });
  await server.start();
  return {
    url: `postgres://postgres:postgres@127.0.0.1:${port}/postgres`,
    db,
    close: async () => {
      await server.stop();
      await db.close();
    },
  };
}
