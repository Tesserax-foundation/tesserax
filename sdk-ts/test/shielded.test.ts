// Private transactions. The scenario runs against a development chain:
//   tesserax --dev --tmp --mine --author <address>
//   TESSERAX_URL=ws://127.0.0.1:9944 npm test

import assert from "node:assert/strict";
import { test } from "node:test";
import { Amount, isShieldedAddress, Keypair, loadShielded, ShieldedAccount, shieldedMinimumFee, TesseraxClient } from "../src/index.js";

const url = process.env.TESSERAX_URL;

test("shielded addresses derive from the key and states roundtrip", async () => {
  await loadShielded();
  const alice = Keypair.fromUri("//Alice");
  const account = await ShieldedAccount.create(alice);
  assert.ok(account.address.startsWith("zte"));
  assert.ok(isShieldedAddress(account.address));
  assert.ok(!isShieldedAddress(alice.address));
  assert.ok(!isShieldedAddress(account.address.slice(0, -1) + (account.address.endsWith("a") ? "b" : "a")));
  // The recovery phrase restores the same address.
  assert.equal((await ShieldedAccount.create(Keypair.fromUri("//Alice"))).address, account.address);
  assert.notEqual((await ShieldedAccount.create(Keypair.fromUri("//Bob"))).address, account.address);

  const restored = await ShieldedAccount.restore(alice, account.state());
  assert.equal(restored.address, account.address);
  assert.equal(restored.balance.planck, 0n);
  assert.equal(restored.block, undefined);
  await assert.rejects(ShieldedAccount.restore(alice, new Uint8Array(10)));
});

test("shield, send privately and unshield on a development chain", { skip: !url && "set TESSERAX_URL to a development node", timeout: 900_000 }, async () => {
  const client = new TesseraxClient(url);
  try {
    const alice = Keypair.fromUri("//Alice");
    const bob = Keypair.fromUri("//Bob");
    const fee = await shieldedMinimumFee(client);
    assert.ok(fee.planck > 0n);

    const alicePrivate = await ShieldedAccount.create(alice);
    const bobPrivate = await ShieldedAccount.create(bob);
    await alicePrivate.sync(client);
    // Earlier runs on the same chain may have left notes.
    const start = alicePrivate.balance.planck;

    await alicePrivate.shield(client, alice, Amount.fromTess("5"));
    await alicePrivate.sync(client);
    assert.equal(alicePrivate.balance.planck, start + Amount.fromTess("5").planck);

    // The state of a synchronized account restores where it stopped.
    const restored = await ShieldedAccount.restore(alice, alicePrivate.state());
    assert.equal(restored.block, alicePrivate.block);
    assert.equal(restored.balance.planck, alicePrivate.balance.planck);

    await bobPrivate.sync(client);
    const bobStart = bobPrivate.balance.planck;
    await alicePrivate.send(client, bobPrivate.address, Amount.fromTess("2"));
    assert.ok(alicePrivate.pending.planck > 0n);
    await alicePrivate.sync(client);
    assert.equal(alicePrivate.pending.planck, 0n);
    assert.equal(alicePrivate.balance.planck, start + Amount.fromTess("3").planck - fee.planck);

    const synced = await bobPrivate.sync(client);
    assert.equal(synced.received.planck, Amount.fromTess("2").planck);
    assert.equal(bobPrivate.balance.planck, bobStart + Amount.fromTess("2").planck);

    const before = (await client.balance(bob.address)).free.planck;
    await bobPrivate.unshield(client, bob.address, Amount.fromTess("1"));
    await bobPrivate.sync(client);
    assert.equal(bobPrivate.balance.planck, bobStart + Amount.fromTess("1").planck - fee.planck);
    assert.equal((await client.balance(bob.address)).free.planck, before + Amount.fromTess("1").planck);

    // More than the balance is refused before anything is submitted.
    await assert.rejects(bobPrivate.send(client, alicePrivate.address, Amount.fromTess("1000")));
  } finally {
    client.destroy();
  }
});
