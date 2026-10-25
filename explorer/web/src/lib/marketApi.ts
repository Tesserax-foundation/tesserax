/** The public market API of the explorer, in the formats price aggregators read: CoinGecko
 * (`/api/coingecko/pairs`, `tickers`, `orderbook`, `historical_trades`) and the supply of TESS
 * (`/api/supply`, `/api/supply/total`, `circulating` and `max`). It answers from the functions
 * of the explorer database (`supabase/migrations/*_market.sql`). Shared by the serverless
 * functions of `api/` and their tests. */

/** Calls a function of the explorer database. */
export type Rpc = (name: string, args?: Record<string, unknown>) => Promise<unknown>;

export interface Answer {
  status: number;
  /** JSON, or plain text for a number of the supply. */
  body: unknown;
  text?: boolean;
}

const TICKER = /^TESS_[A-Z]{3,5}$/;

const fail = (status: number, error: string): Answer => ({ status, body: { error } });

/** A positive integer parameter, or `fallback`. */
function integer(value: string | null, fallback: number): number | undefined {
  if (value === null || value === "") return fallback;
  if (!/^\d+$/.test(value)) return undefined;
  return Number(value);
}

/** A unix time in seconds or in milliseconds, as an ISO date. */
function time(value: string | null): string | null | undefined {
  if (value === null || value === "") return null;
  if (!/^\d+$/.test(value)) return undefined;
  const n = Number(value);
  return new Date(n > 1e12 ? n : n * 1000).toISOString();
}

/** The CoinGecko endpoint `endpoint` with the query `params`. */
export async function coingecko(endpoint: string, params: URLSearchParams, rpc: Rpc): Promise<Answer> {
  switch (endpoint) {
    case "pairs":
      return { status: 200, body: await rpc("market_pairs") };
    case "tickers":
      return { status: 200, body: await rpc("market_tickers") };
    case "orderbook": {
      const ticker = params.get("ticker_id") ?? "";
      if (!TICKER.test(ticker)) return fail(400, "ticker_id is required, such as TESS_USDC");
      const depth = integer(params.get("depth"), 100);
      if (depth === undefined) return fail(400, "depth is a number of levels");
      return { status: 200, body: await rpc("market_orderbook", { ticker_id: ticker, depth: depth === 0 ? 500 : depth }) };
    }
    case "historical_trades": {
      const ticker = params.get("ticker_id") ?? "";
      if (!TICKER.test(ticker)) return fail(400, "ticker_id is required, such as TESS_USDC");
      const kind = params.get("type");
      if (kind !== null && kind !== "buy" && kind !== "sell") return fail(400, "type is buy or sell");
      const limit = integer(params.get("limit"), 500);
      const start = time(params.get("start_time"));
      const end = time(params.get("end_time"));
      if (limit === undefined || start === undefined || end === undefined) return fail(400, "limit, start_time and end_time are numbers");
      return {
        status: 200,
        body: await rpc("market_trades", { ticker_id: ticker, kind, max_rows: limit === 0 ? 1000 : limit, start_time: start, end_time: end }),
      };
    }
    default:
      return fail(404, "endpoints: pairs, tickers, orderbook, historical_trades");
  }
}

/** The supply of TESS: all of it as JSON, or one number as plain text. */
export async function supply(kind: string | undefined, rpc: Rpc): Promise<Answer> {
  const values = (await rpc("market_supply")) as { total: string; circulating: string; max: string };
  if (!kind) return { status: 200, body: values };
  if (kind === "total" || kind === "circulating" || kind === "max") return { status: 200, body: values[kind], text: true };
  return fail(404, "supplies: total, circulating, max");
}

/** Calls through the REST API of the Supabase project, with its public anon key. */
export function supabaseRpc(url: string, key: string): Rpc {
  return async (name, args = {}) => {
    const response = await fetch(`${url.replace(/\/$/, "")}/rest/v1/rpc/${name}`, {
      method: "POST",
      headers: { apikey: key, authorization: `Bearer ${key}`, "content-type": "application/json" },
      body: JSON.stringify(args),
    });
    if (!response.ok) throw new Error(`${name}: ${response.status} ${await response.text()}`);
    return response.json();
  };
}

/** The HTTP response of `answer`: public, cached briefly at the edge. */
export function respond(answer: Answer): Response {
  const headers = {
    "access-control-allow-origin": "*",
    "cache-control": answer.status === 200 ? "public, s-maxage=30, stale-while-revalidate=60" : "no-store",
    "content-type": answer.text ? "text/plain; charset=utf-8" : "application/json",
  };
  return new Response(answer.text ? String(answer.body) : JSON.stringify(answer.body), { status: answer.status, headers });
}

/** The database of the deployment, or an answer saying it has none. */
export function deploymentRpc(env: Record<string, string | undefined>): Rpc | Answer {
  const url = env.SUPABASE_URL ?? env.VITE_SUPABASE_URL;
  const key = env.SUPABASE_ANON_KEY ?? env.VITE_SUPABASE_ANON_KEY;
  if (!url || !key) return fail(503, "the explorer database is not configured");
  return supabaseRpc(url, key);
}
