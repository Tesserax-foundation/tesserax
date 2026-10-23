// Smart contracts against a running development chain: deploys sdk/fixtures/contracts/Token.sol.
//   tesserax --dev --tmp --mine --author <address>
//   TESSERAX_URL=ws://127.0.0.1:9944 npm test

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { ContractReverted, Keypair, TesseraxClient, toH160, type ContractEvent } from "../src/index.js";

const url = process.env.TESSERAX_URL;
const fixtures = new URL("../../sdk/fixtures/contracts/", import.meta.url);

test("toH160 maps accounts as the runtime does", () => {
  // Accounts derived from an Ethereum address map back to it.
  const derived = new Uint8Array(32).fill(0xee);
  derived.set(Uint8Array.from({ length: 20 }, (_, i) => i + 1));
  assert.equal(toH160(derived).toLowerCase(), "0x0102030405060708090a0b0c0d0e0f1011121314");
});

test("a token on a development chain", { skip: !url && "set TESSERAX_URL to a development node", timeout: 900_000 }, async () => {
  const client = new TesseraxClient(url);
  try {
    const alice = Keypair.fromUri("//Alice");
    const bob = Keypair.fromUri("//Bob");
    const mapped = await client.api.apis.ReviveApi.address(alice.address, { at: "best" });
    assert.equal(toH160(alice.address).toLowerCase(), String(mapped).toLowerCase());

    const code = new Uint8Array(readFileSync(new URL("Token.polkavm", fixtures)));
    const abi = JSON.parse(readFileSync(new URL("Token.abi.json", fixtures), "utf8"));
    const token = await client.deployContract(alice, { code, abi, args: ["Tesserax Coin", 1_000_000n] });
    const minted = token.deployment.contractEvents[0];
    assert.equal(minted.name, "Transfer");
    assert.equal(minted.args.to, toH160(alice.address));
    assert.equal(minted.args.value, 1_000_000n);

    assert.equal(await token.read("name"), "Tesserax Coin");
    assert.equal(await token.read("balanceOf", [toH160(alice.address)]), 1_000_000n);

    const seen = new Promise<ContractEvent>((resolve, reject) => {
      const stop = token.watchEvents(
        (event) => {
          stop();
          resolve(event);
        },
        reject,
      );
    });

    const sent = await token.write(alice, "transfer", [toH160(bob.address), 250n]);
    assert.deepEqual(sent.contractEvents[0].args, { from: toH160(alice.address), to: toH160(bob.address), value: 250n });
    assert.equal(await token.read("balanceOf", [toH160(bob.address)]), 250n);
    const watched = await seen;
    assert.equal(watched.name, "Transfer");
    assert.equal(watched.blockNumber, sent.blockNumber);

    await assert.rejects(token.write(bob, "transfer", [toH160(alice.address), 1_000n]), (error) => {
      assert.ok(error instanceof ContractReverted);
      assert.equal(error.reason, "InsufficientBalance(available: 250, required: 1000)");
      return true;
    });
    await assert.rejects(token.write(bob, "burn", [1_000n]), (error) => {
      assert.ok(error instanceof ContractReverted);
      assert.equal(error.reason, "burn exceeds balance");
      return true;
    });

    const events = await token.events(token.deployment.blockNumber, sent.blockNumber);
    assert.deepEqual(
      events.filter((e) => e.name === "Transfer").map((e) => e.args.value),
      [1_000_000n, 250n],
    );

    // The same contract, from its address and ABI.
    assert.equal(await client.contract(token.address, abi).read("totalSupply"), 1_000_000n);
  } finally {
    client.destroy();
  }
});
