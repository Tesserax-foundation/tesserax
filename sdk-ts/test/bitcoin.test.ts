// The Bitcoin side of swaps. The transactions built here are checked by Bitcoin Core in the tests
// of the Rust SDK (`sdk/src/swaps/testdata/bitcoin-ts.json`); WRITE_VECTORS=1 writes them again.

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { test } from "node:test";
import { secp256k1 } from "@noble/curves/secp256k1.js";
import { sha256 } from "@noble/hashes/sha2.js";
import * as btc from "@scure/btc-signer";
import {
  NOTICE_SATS,
  addressOfKey,
  claimTransaction,
  decodeAnnouncement,
  encodeAnnouncement,
  findLock,
  makerKey,
  extractSigned,
  htlcOutputScript,
  htlcScript,
  lockPsbt,
  outpointBytes,
  refundKey,
  refundTransaction,
  type Htlc,
} from "../src/bitcoin.js";
import { Keypair } from "../src/index.js";

const VECTORS = new URL("../../sdk/src/swaps/testdata/bitcoin-ts.json", import.meta.url);
const hex = (b: Uint8Array) => Buffer.from(b).toString("hex");
const key = (byte: number) => new Uint8Array(32).fill(byte);

test("payments locked by a browser wallet, taken back with the key of the Tesserax wallet", () => {
  const maker = secp256k1.getPublicKey(key(1), true);
  const payerKey = key(5);
  const payerPub = secp256k1.getPublicKey(payerKey, true);
  const payer = addressOfKey(payerPub, "testnet4");
  assert.match(payer, /^tb1q/);

  const secret = new Uint8Array(32).fill(7);
  const hash = sha256(secret);
  const refund = refundKey(new Uint8Array(32).fill(9), hash);
  assert.deepEqual(refundKey(new Uint8Array(32).fill(9), hash), refund, "derived again from the seed");
  assert.notDeepEqual(refundKey(new Uint8Array(32).fill(9), sha256(hash)).publicKey, refund.publicKey);
  const htlc: Htlc = { hash, maker, taker: refund.publicKey, timelock: 1_800_000_000 };
  const taker = Keypair.fromUri("//Alice").address;
  const announcement = { offer: 4, taker, hash, timelock: htlc.timelock, refund: refund.publicKey };
  assert.equal(encodeAnnouncement(announcement).length, 111);

  // The output the browser wallet pays from.
  const funding = new btc.Transaction({ allowUnknownInputs: true });
  funding.addOutput({ script: btc.p2wpkh(payerPub).script, amount: 100_000n });
  funding.addInput({ txid: new Uint8Array(32), index: 0, finalScriptSig: new Uint8Array([0x51]) });
  const fundingRaw = funding.toBytes(true, false);

  const built = lockPsbt({
    htlc,
    announcement,
    amount: 40_000n,
    payer,
    utxos: [{ txid: funding.id, vout: 0, amount: 100_000n, rawTx: fundingRaw }],
    feeRate: 2,
    network: "testnet4",
  });
  assert.deepEqual(built.inputs, [0]);

  // What the browser wallet does: sign its inputs, and only them.
  const wallet = btc.Transaction.fromPSBT(built.psbt, { allowUnknownOutputs: true });
  for (const i of built.inputs) wallet.signIdx(payerKey, i);
  const signed = extractSigned(wallet.toPSBT(0));
  assert.equal(signed.txid, built.txid, "signatures do not change the transaction id");

  const lockTx = btc.Transaction.fromRaw(signed.raw, { allowUnknownOutputs: true });
  assert.equal(lockTx.outputsLength, 4);
  assert.deepEqual(lockTx.getOutput(0).script, htlcOutputScript(htlc));
  assert.equal(lockTx.getOutput(1).amount, NOTICE_SATS);
  const fee = 100_000n - 40_000n - NOTICE_SATS - lockTx.getOutput(3).amount!;
  assert.equal(fee, built.fee);
  assert.equal(outpointBytes(built.txid, 0).length, 36);

  // The maker finds the payment, with the announced refund key, and claims it with the secret,
  // sweeping the notice to its key.
  const found = findLock(signed.raw, maker)!;
  assert.ok(found);
  assert.deepEqual([found.offer, found.taker, found.vout, found.amount], [4, taker, 0, 40_000n]);
  assert.deepEqual(found.htlc, htlc);
  assert.equal(findLock(signed.raw, secp256k1.getPublicKey(key(3), true)), undefined);
  const claimRaw = claimTransaction(found, secret, key(1), payer, 2, "testnet4", [{ txid: built.txid, vout: 1, amount: NOTICE_SATS }]);
  const claimTx = btc.Transaction.fromRaw(claimRaw, { allowUnknownInputs: true, allowUnknownOutputs: true });
  assert.equal(claimTx.inputsLength, 2);

  const refundRaw = refundTransaction({ txid: built.txid, vout: 0, amount: 40_000n, htlc }, refund.secretKey, payer, 2, "testnet4");
  const refundTx = btc.Transaction.fromRaw(refundRaw, { allowUnknownInputs: true, allowUnknownOutputs: true });
  assert.equal(refundTx.lockTime, htlc.timelock);

  const vectors = {
    script: hex(htlcScript(htlc)),
    maker: hex(maker),
    refund: hex(refund.publicKey),
    payer_script: hex(btc.p2wpkh(payerPub).script),
    funding_amount: 100_000,
    lock: hex(signed.raw),
    refund_tx: hex(refundRaw),
    claim_tx: hex(claimRaw),
    taker: hex(Keypair.fromUri("//Alice").accountId),
  };
  if (process.env.WRITE_VECTORS) writeFileSync(VECTORS, JSON.stringify(vectors, null, 2) + "\n");
  assert.deepEqual(JSON.parse(readFileSync(VECTORS, "utf8")), vectors);
});

test("payments announced without refund key, by a wallet holding its key, are found", () => {
  const maker = secp256k1.getPublicKey(key(1), true);
  const payerKey = key(6);
  const payerPub = secp256k1.getPublicKey(payerKey, true);
  const hash = sha256(new Uint8Array(32).fill(8));
  const htlc: Htlc = { hash, maker, taker: payerPub, timelock: 1_800_000_000 };
  const taker = Keypair.fromUri("//Bob").address;
  // TX: the announcement of 78 bytes, the refund key being the key of the first input.
  const data = encodeAnnouncement({ offer: 7, taker, hash, timelock: htlc.timelock, refund: new Uint8Array(33).fill(2) }).slice(0, 78);
  data.set(new TextEncoder().encode("TX"), 0);
  assert.deepEqual(decodeAnnouncement(data)?.refund, undefined);
  const funding = new btc.Transaction({ allowUnknownInputs: true });
  funding.addOutput({ script: btc.p2wpkh(payerPub).script, amount: 60_000n });
  funding.addInput({ txid: new Uint8Array(32), index: 0, finalScriptSig: new Uint8Array([0x51]) });
  const tx = new btc.Transaction({ allowUnknownOutputs: true });
  tx.addInput({ txid: funding.id, index: 0, witnessUtxo: { script: btc.p2wpkh(payerPub).script, amount: 60_000n } });
  tx.addOutput({ script: htlcOutputScript(htlc), amount: 20_000n });
  tx.addOutput({ script: btc.p2wpkh(maker).script, amount: NOTICE_SATS });
  tx.addOutput({ script: btc.Script.encode(["RETURN", data]), amount: 0n });
  tx.signIdx(payerKey, 0);
  tx.finalize();
  const found = findLock(tx.extract(), maker)!;
  assert.deepEqual([found.offer, found.taker, found.amount], [7, taker, 20_000n]);
  assert.deepEqual(found.htlc, htlc);
  // The maker key of an account is derived again from its seed.
  assert.deepEqual(makerKey(new Uint8Array(32).fill(9)), makerKey(new Uint8Array(32).fill(9)));
  assert.notDeepEqual(makerKey(new Uint8Array(32).fill(9)).publicKey, makerKey(new Uint8Array(32).fill(8)).publicKey);
});
