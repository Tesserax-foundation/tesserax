// Against a running development chain:
//   tesserax --dev --tmp --mine --author <address>
//   TESSERAX_URL=ws://127.0.0.1:9944 npm test

import assert from "node:assert/strict";
import { test } from "node:test";
import { Amount, Keypair, Tasks, TesseraxClient, TransactionFailed, blocksFor } from "../src/index.js";

const url = process.env.TESSERAX_URL;
const tess = (text: string) => Amount.fromTess(text);

test("agents trade work through the task market", { skip: !url && "set TESSERAX_URL to a development node", timeout: 900_000 }, async () => {
  const client = new TesseraxClient(url);
  const tasks = new Tasks(client);
  const owner = Keypair.fromUri("//Alice");
  const worker = Keypair.fromUri("//Bob");
  const { keypair: agent } = Keypair.generate();
  try {
    await client.transfer(owner, agent.address, tess("1"));
    await client.authorizeAgent(owner, agent.address, { budget: tess("10"), maxPerCall: tess("5") });

    const id = await tasks.create(agent, owner.address, {
      spec: "Translate https://example.com/contract-fr.txt to English",
      capability: "translation",
      reward: tess("2"),
      bond: tess("0.1"),
      deliveryBlocks: blocksFor(30),
      reviewBlocks: blocksFor(30),
    });
    const open = await tasks.open("translation");
    assert.ok(open.some((t) => t.id === id && t.client === owner.address));

    await tasks.accept(worker, undefined, id);
    await tasks.deliver(worker, undefined, id, "ipfs://contract-en");
    const delivered = await tasks.task(id);
    assert.equal(delivered?.status.state, "delivered");

    await assert.rejects(tasks.dispute(agent, owner.address, id), (error) => {
      assert.ok(error instanceof TransactionFailed);
      return true;
    });

    const before = (await client.balance(worker.address)).free.planck;
    await tasks.approve(agent, owner.address, id);
    assert.equal((await client.balance(worker.address)).free.planck - before, tess("2.1").planck);
    assert.equal(await tasks.task(id), undefined);

    await client.revokeAgent(owner, agent.address);
  } finally {
    client.destroy();
  }
});
