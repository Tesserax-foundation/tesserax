import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Amount, Keypair, formatAddress, parseAddress, verify } from "../src/index.js";
import { toHex } from "../src/keys.js";

const vectors: { uri: string; account_id: string; address: string; public_key: string }[] = JSON.parse(
  readFileSync(new URL("../../sdk/vectors/keys.json", import.meta.url), "utf8"),
).vectors;

for (const vector of vectors) {
  test(`key of ${vector.uri.slice(0, 40)} matches the node`, () => {
    const key = Keypair.fromUri(vector.uri);
    assert.equal(toHex(key.publicKey), vector.public_key);
    assert.equal(toHex(key.accountId), vector.account_id);
    assert.equal(key.address, vector.address);
  });
}

test("generated keys roundtrip", () => {
  const { keypair, phrase } = Keypair.generate();
  assert.equal(phrase.split(" ").length, 24);
  assert.equal(Keypair.fromUri(phrase).address, keypair.address);
  assert.notEqual(Keypair.fromUri(`${phrase}//agent`).address, keypair.address);
});

test("invalid keys are refused", () => {
  for (const uri of ["not a phrase", "//Alice/soft", "0x1234"]) {
    assert.throws(() => Keypair.fromUri(uri));
  }
});

test("signatures", () => {
  const key = Keypair.fromUri("//Alice");
  const message = new TextEncoder().encode("message");
  const signature = key.sign(message);
  assert.equal(signature.length, 3309);
  assert.ok(verify(key.publicKey, message, signature));
  assert.ok(!verify(key.publicKey, new TextEncoder().encode("other"), signature));
  assert.ok(!verify(key.publicKey, message, signature, new TextEncoder().encode("other")));
  // Hedged: two signatures of the same message differ.
  assert.notDeepEqual(key.sign(message), signature);
});

test("addresses", () => {
  const key = Keypair.fromUri("//Alice");
  assert.deepEqual(parseAddress(key.address), key.accountId);
  assert.deepEqual(parseAddress(toHex(key.accountId)), key.accountId);
  assert.equal(formatAddress(key.accountId), key.address);
  for (const bad of ["15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5", "0x1234", "garbage"]) {
    assert.throws(() => parseAddress(bad), bad);
  }
});

test("amounts", () => {
  assert.equal(Amount.fromTess("1").planck, 10n ** 12n);
  assert.equal(Amount.fromTess("1.5").planck, 1_500_000_000_000n);
  assert.equal(Amount.fromTess("0.000000000001").planck, 1n);
  assert.equal(Amount.fromTess(".25").planck, 250_000_000_000n);
  assert.equal(new Amount(1_500_000_000_000n).toString(), "1.5");
  assert.equal(new Amount(14_000_000n * 10n ** 12n).toString(), "14000000");
  assert.equal(JSON.stringify({ a: new Amount(1n) }), '{"a":"0.000000000001"}');
  for (const bad of ["", ".", "-1", "1.0000000000001", "1e3", "abc"]) {
    assert.throws(() => Amount.fromTess(bad), bad);
  }
});
