import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Amount, Keypair, Voucher, parsePaymentRequired, paymentRequiredHeader, voucherMessage } from "../src/index.js";
import { fromHex, toHex } from "../src/keys.js";

const vector = JSON.parse(readFileSync(new URL("../../sdk/vectors/voucher.json", import.meta.url), "utf8"));

test("vouchers match the Rust SDK", () => {
  const genesis = fromHex(vector.genesis);
  const message = voucherMessage(genesis, BigInt(vector.channel), Amount.fromTess(vector.amount));
  assert.equal(toHex(message), vector.message);
  const voucher = Voucher.fromHeader(vector.header);
  const signer = Keypair.fromUri(vector.signer);
  assert.ok(voucher.verify(genesis, signer.publicKey));
  assert.ok(!voucher.verify(new Uint8Array(32), signer.publicKey));
});

test("vouchers roundtrip and bind the amount", () => {
  const key = Keypair.fromUri("//Alice");
  const genesis = new Uint8Array(32).fill(3);
  const voucher = Voucher.sign(key, genesis, 12n, Amount.fromTess("0.037"));
  const parsed = Voucher.fromHeader(voucher.toHeader());
  assert.ok(parsed.verify(genesis, key.publicKey));
  const tampered = new Voucher(parsed.channel, Amount.fromTess("0.038"), parsed.signature);
  assert.ok(!tampered.verify(genesis, key.publicKey));
  assert.throws(() => Voucher.fromHeader('{"channel":-1,"amount":"1","signature":""}'));
});

test("payment requirements roundtrip", () => {
  const header = paymentRequiredHeader({ payee: "te1", price: Amount.fromTess("0.001"), genesis: "0x00", accepted: Amount.fromTess("0.5") });
  const parsed = parsePaymentRequired(header);
  assert.equal(parsed.price.toString(), "0.001");
  assert.equal(parsed.accepted?.toString(), "0.5");
  assert.equal(parsed.error, undefined);
});
