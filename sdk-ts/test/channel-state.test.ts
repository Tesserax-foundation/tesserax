import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  Amount,
  ChannelPayer,
  ChannelReceiver,
  Keypair,
  StorageError,
  Voucher,
  parsePaymentRequired,
  type ChannelInfo,
} from "../src/index.js";

// State files of the channel payer and receiver, against a chain simulated in memory.

const PAYER = Keypair.fromUri("//Alice");
const SERVICE = Keypair.fromUri("//Bob");
const PRICE = Amount.fromTess("0.1");
const tess = (text: string) => Amount.fromTess(text);
const options = { deposit: tess("2"), maxPrice: tess("1") };

/** The client and channel calls used by the payer and the receiver, on a chain in memory. */
class FakeChain {
  readonly channels = new Map<bigint, ChannelInfo>();
  opened = 0;
  claims = 0;

  constructor(readonly genesis = new Uint8Array(32).fill(7)) {}

  // TesseraxClient
  async genesisHash() {
    return this.genesis;
  }
  async bestBlockNumber() {
    return 1;
  }

  // Channels
  async channel(id: bigint) {
    const c = this.channels.get(id);
    return c && { ...c };
  }
  async open(payer: Keypair, payee: string, deposit: Amount) {
    const id = BigInt(this.channels.size);
    this.channels.set(id, {
      id,
      payer: payer.address,
      payee,
      signer: payer.address,
      signerPublic: payer.publicKey,
      deposit,
      claimed: new Amount(0n),
    });
    this.opened++;
    return id;
  }
  async topUp(_signer: Keypair, _owner: string | undefined, id: bigint, amount: Amount) {
    const c = this.channels.get(id)!;
    c.deposit = new Amount(c.deposit.planck + amount.planck);
    return {} as any;
  }
  async claim(_payee: Keypair, voucher: Voucher) {
    const c = this.channels.get(voucher.channel)!;
    assert.ok(voucher.verify(this.genesis, c.signerPublic));
    assert.ok(c.claimed.planck < voucher.amount.planck, "claimed twice");
    c.claimed = voucher.amount;
    this.claims++;
    return "claimed" as any;
  }
}

const withChain = <T extends ChannelPayer | ChannelReceiver>(object: T, chain: FakeChain): T => {
  (object as any).ops = chain;
  return object;
};
const payer = async (chain: FakeChain, file?: string, key = PAYER, owner?: string) =>
  withChain(
    file ? await ChannelPayer.withFile(chain as any, key, { ...options, owner }, file) : new ChannelPayer(chain as any, key, options),
    chain,
  );
const receiver = async (chain: FakeChain, file?: string, key = SERVICE) =>
  withChain(file ? await ChannelReceiver.withFile(chain as any, key, file) : new ChannelReceiver(chain as any, key), chain);
const pay = async (p: ChannelPayer, r: ChannelReceiver) => p.pay(parsePaymentRequired(await r.paymentRequired(PRICE)));
const directory = () => mkdtempSync(join(tmpdir(), "tesserax-channels-"));

test("a payer resumes its channels after a restart", async () => {
  const chain = new FakeChain();
  const file = join(directory(), "payer.json");
  const service = await receiver(chain);
  let p = await payer(chain, file);
  await pay(p, service);
  const second = await pay(p, service);

  p = await payer(chain, file);
  const third = await pay(p, service);
  assert.equal(chain.opened, 1);
  assert.equal(third.channel, second.channel);
  assert.equal(third.amount.toString(), "0.3");

  const state = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(state.kind, "payer");
  assert.deepEqual(state.channels[SERVICE.address], { id: Number(third.channel), deposit: "2", spent: "0.3" });
  if (process.platform !== "win32") assert.equal(statSync(file).mode & 0o777, 0o600);
});

test("a payer records a voucher before returning it", async () => {
  const chain = new FakeChain();
  const dir = directory();
  const service = await receiver(chain);
  const p = await payer(chain, join(dir, "payer.json"));
  await pay(p, service);
  // A state file that cannot be written: its directory is gone.
  (p as any).file = join(dir, "missing", "payer.json");
  await assert.rejects(pay(p, service), StorageError);
  assert.equal(p.channels.get(SERVICE.address)!.spent.toString(), "0.1");
});

test("a service claims after a restart and refuses replays", async () => {
  const chain = new FakeChain();
  const file = join(directory(), "service.json");
  const p = await payer(chain);
  let service = await receiver(chain, file);
  const first = await pay(p, service);
  assert.deepEqual(await service.verify(first.toHeader(), PRICE), { ok: true, amount: tess("0.1") });
  const second = await pay(p, service);
  assert.deepEqual(await service.verify(second.toHeader(), PRICE), { ok: true, amount: tess("0.2") });

  service = await receiver(chain, file);
  assert.equal(service.unclaimed().toString(), "0.2");
  const replay = await service.verify(second.toHeader(), PRICE);
  assert.deepEqual(replay, { ok: false, rejection: { reason: "the voucher does not pay the price", accepted: tess("0.2") } });

  const third = await pay(p, service);
  assert.deepEqual(await service.verify(third.toHeader(), PRICE), { ok: true, amount: tess("0.3") });
  assert.deepEqual(await service.claimAll(), ["claimed"]);
  assert.equal(chain.channels.get(third.channel)!.claimed.toString(), "0.3");

  service = await receiver(chain, file);
  assert.equal(service.unclaimed().toString(), "0");
  assert.deepEqual(await service.claimAll(), []);
});

test("a claim interrupted by a crash is not repeated", async () => {
  const chain = new FakeChain();
  const file = join(directory(), "service.json");
  const p = await payer(chain);
  let service = await receiver(chain, file);
  const voucher = await pay(p, service);
  await service.verify(voucher.toHeader(), PRICE);
  // The claim went through, then the service stopped before recording it.
  await chain.claim(SERVICE, voucher);

  service = await receiver(chain, file);
  assert.deepEqual(await service.claimAll(), []);
  assert.equal(chain.claims, 1);
  assert.equal(service.unclaimed().toString(), "0");
});

test("vouchers of closed channels are dropped", async () => {
  const chain = new FakeChain();
  const file = join(directory(), "service.json");
  const p = await payer(chain);
  const service = await receiver(chain, file);
  const voucher = await pay(p, service);
  await service.verify(voucher.toHeader(), PRICE);
  chain.channels.delete(voucher.channel);
  assert.deepEqual(await service.claimAll(), []);
  assert.deepEqual(JSON.parse(readFileSync(file, "utf8")).channels, {});
});

test("state files belong to one chain and key", async () => {
  const chain = new FakeChain();
  const file = join(directory(), "state.json");
  await pay(await payer(chain, file), await receiver(chain));

  await assert.rejects(payer(chain, file, SERVICE), /another key/);
  await assert.rejects(payer(chain, file, PAYER, SERVICE.address), /another owner/);
  await assert.rejects(payer(new FakeChain(new Uint8Array(32)), file), /another chain/);
  await assert.rejects(receiver(chain, file), /not a receiver state file/);

  writeFileSync(file, "{not json");
  await assert.rejects(payer(chain, file), StorageError);
});
