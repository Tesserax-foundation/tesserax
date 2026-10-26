// One number of the supply of TESS, as plain text: /api/supply/total, circulating or max.

import { deploymentRpc, respond, supply } from "../../src/lib/marketApi.js";

declare const process: { env: Record<string, string | undefined> };

export async function GET(request: Request): Promise<Response> {
  const rpc = deploymentRpc(process.env);
  if (typeof rpc !== "function") return respond(rpc);
  try {
    return respond(await supply(new URL(request.url).pathname.split("/").pop(), rpc));
  } catch {
    return respond({ status: 502, body: { error: "the explorer database does not answer" } });
  }
}
