// CoinGecko market data of TESS: /api/coingecko/pairs, tickers, orderbook, historical_trades.

import { coingecko, deploymentRpc, respond } from "../../src/lib/marketApi";

declare const process: { env: Record<string, string | undefined> };

export async function GET(request: Request): Promise<Response> {
  const url = new URL(request.url);
  const rpc = deploymentRpc(process.env);
  if (typeof rpc !== "function") return respond(rpc);
  try {
    return respond(await coingecko(url.pathname.split("/").pop() ?? "", url.searchParams, rpc));
  } catch {
    return respond({ status: 502, body: { error: "the explorer database does not answer" } });
  }
}
