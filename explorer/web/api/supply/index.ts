// The supply of TESS: total, circulating and maximum, as JSON.

import { deploymentRpc, respond, supply } from "../../src/lib/marketApi";

declare const process: { env: Record<string, string | undefined> };

export async function GET(): Promise<Response> {
  const rpc = deploymentRpc(process.env);
  if (typeof rpc !== "function") return respond(rpc);
  try {
    return respond(await supply(undefined, rpc));
  } catch {
    return respond({ status: 502, body: { error: "the explorer database does not answer" } });
  }
}
