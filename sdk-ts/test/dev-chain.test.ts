// Against a running development chain:
//   tesserax --dev --tmp --mine --author <address>
//   TESSERAX_URL=ws://127.0.0.1:9944 npm test

import assert from "node:assert/strict";
import { test } from "node:test";
import { Amount, Keypair, TesseraxClient, TransactionFailed } from "../src/index.js";

const url = process.env.TESSERAX_URL;
const tess = (text: string) => Amount.fromTess(text);

test("agents on a development chain", { skip: !url && "set TESSERAX_URL to a development node", timeout: 900_000 }, async () => {
  const client = new TesseraxClient(url);
  try {
    const info = await client.chainInfo();
    assert.equal(info.ss58Prefix, 2261);

    const owner = Keypair.fromUri("//Alice");
    const merchant = Keypair.fromUri("//Bob");
    const { keypair: agent } = Keypair.generate();

    await client.transfer(owner, agent.address, tess("1"));
    await client.authorizeAgent(owner, agent.address, { budget: tess("10"), maxPerCall: tess("6") });
    assert.equal((await client.delegation(owner.address, agent.address))?.remaining.toString(), "10");

    const before = (await client.balance(merchant.address)).free.planck;
    await client.agentTransfer(agent, owner.address, merchant.address, tess("5"));
    assert.equal((await client.balance(merchant.address)).free.planck - before, tess("5").planck);
    assert.equal((await client.delegation(owner.address, agent.address))?.remaining.toString(), "5");

    await assert.rejects(client.agentTransfer(agent, owner.address, merchant.address, tess("6")), (error) => {
      assert.ok(error instanceof TransactionFailed);
      assert.equal(error.error, "Agents::BudgetExceeded");
      return true;
    });

    if (!(await client.agent(merchant.address))) {
      await client.registerAgent(merchant, {
        name: "translator",
        description: "French to English",
        endpoint: "https://translator.example/mcp",
        capabilities: ["translation"],
      });
    }
    const found = await client.findAgents("translation");
    assert.ok(found.some((a) => a.address === merchant.address));

    const payments = (await client.agent(merchant.address))!.reputation.payments;
    await client.agentPayAgent(agent, owner.address, merchant.address, tess("1"));
    assert.equal((await client.agent(merchant.address))!.reputation.payments, payments + 1n);

    await client.revokeAgent(owner, agent.address);
    assert.equal(await client.delegation(owner.address, agent.address), undefined);
  } finally {
    client.destroy();
  }
});
