// Against a running development chain:
//   tesserax --dev --tmp --mine --author <address>
//   TESSERAX_URL=ws://127.0.0.1:9944 npm test

import assert from "node:assert/strict";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { test } from "node:test";
import { Amount, ChannelPayer, ChannelReceiver, Keypair, PAYMENT_HEADER, TesseraxClient, VOUCHER_HEADER } from "../src/index.js";

const url = process.env.TESSERAX_URL;
const tess = (text: string) => Amount.fromTess(text);

test("an agent pays a service per request", { skip: !url && "set TESSERAX_URL to a development node", timeout: 900_000 }, async () => {
  const client = new TesseraxClient(url);
  const owner = Keypair.fromUri("//Alice");
  const serviceKey = Keypair.fromUri("//Bob");
  const { keypair: agent } = Keypair.generate();

  // A service charging 0.001 TESS per request.
  const receiver = new ChannelReceiver(client, serviceKey);
  const price = tess("0.001");
  let served = 0;
  const server = createServer(async (request, response) => {
    const voucher = request.headers[VOUCHER_HEADER.toLowerCase()];
    const result = typeof voucher === "string" ? await receiver.verify(voucher, price) : undefined;
    if (!result?.ok) {
      response.writeHead(402, { [PAYMENT_HEADER]: await receiver.paymentRequired(price, result?.rejection) });
      return response.end();
    }
    served += 1;
    response.end(`translation number ${served}`);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const service = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;

  try {
    await client.transfer(owner, agent.address, tess("1"));
    await client.authorizeAgent(owner, agent.address, { budget: tess("5"), maxPerCall: tess("3") });

    const payer = new ChannelPayer(client, agent, { owner: owner.address, deposit: tess("2"), maxPrice: tess("0.01") });
    for (let request = 1; request <= 5; request++) {
      const response = await payer.fetch(`${service}/translate`);
      assert.equal(response.status, 200);
      assert.equal(await response.text(), `translation number ${request}`);
    }
    const [channel] = payer.channels.values();
    assert.equal(channel.spent.toString(), "0.005");

    await assert.rejects(payer.fetch(`${service}/translate`, {}, tess("0.0001")), /more than the maximum/);

    assert.equal(receiver.unclaimed().toString(), "0.005");
    const before = (await client.balance(serviceKey.address)).free.planck;
    await receiver.claimAll();
    const fee = before + tess("0.005").planck - (await client.balance(serviceKey.address)).free.planck;
    assert.ok(fee > 0n && fee < tess("0.005").planck, `fee ${new Amount(fee)}`);

    await client.revokeAgent(owner, agent.address);
  } finally {
    server.close();
    client.destroy();
  }
});
