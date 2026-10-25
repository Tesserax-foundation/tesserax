// The swap against a development chain and a local EVM chain with the swap contract deployed
// (`swaps/evm/deploy.sh`):
//   tesserax --dev --tmp --mine --author <address>
//   anvil
//   TESSERAX_URL=ws://127.0.0.1:9944 EVM_URL=http://127.0.0.1:8545 npm test

import assert from "node:assert/strict";
import { test } from "node:test";
import { createWalletClient, http, zeroAddress } from "viem";
import { mnemonicToAccount } from "viem/accounts";
import { Amount, EvmSwaps, Keypair, SWAP_CONTRACT, TesseraxClient, hashOf, newSecret, paymentDue, tessFor, type EvmNetwork } from "../src/index.js";
import { PLANCK_PER_TESS } from "../src/amount.js";

const url = process.env.TESSERAX_URL;
const evmUrl = process.env.EVM_URL;
const hex = (bytes: Uint8Array) => Buffer.from(bytes).toString("hex");

test("payments round up and buy back the amount", () => {
  // 0.05 USDC (6 decimals) per TESS.
  const price = 50_000n;
  assert.equal(paymentDue(100n * PLANCK_PER_TESS, price), 5_000_000n);
  assert.equal(paymentDue(1n, price), 1n);
  for (const amount of [1n, 999n, PLANCK_PER_TESS, 123_456_789_012_345n]) {
    const due = paymentDue(amount, price);
    assert.ok(tessFor(due, price) >= amount);
    assert.ok(tessFor(due - 1n, price) < amount);
  }
  assert.equal(tessFor(1_000n, 0n), 0n);
});

test("secrets hash with SHA-256", () => {
  assert.equal(hex(hashOf(new Uint8Array(32))), "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925");
  const { secret, hash } = newSecret();
  assert.equal(hex(hashOf(secret)), hex(hash));
});

test("a swap of TESS against ETH", { skip: (!url || !evmUrl) && "set TESSERAX_URL and EVM_URL", timeout: 600_000 }, async () => {
  const client = new TesseraxClient(url);
  try {
    const network: EvmNetwork = {
      chain: "Base",
      name: "Local EVM",
      chainId: 31_337,
      rpcUrl: evmUrl!,
      usdc: zeroAddress,
      explorer: "",
      nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
    };
    const mnemonic = "test test test test test test test test test test test junk";
    const evmMaker = mnemonicToAccount(mnemonic, { addressIndex: 3 });
    const evmTaker = mnemonicToAccount(mnemonic, { addressIndex: 4 });
    const evm = new EvmSwaps(network, { walletClient: createWalletClient({ account: evmTaker, transport: http(evmUrl) }) });
    const collector = new EvmSwaps(network, { walletClient: createWalletClient({ account: evmMaker, transport: http(evmUrl) }) });

    // The maker offers 100 TESS at 0.001 ETH.
    const maker = Keypair.fromUri("//Alice");
    const price = 10n ** 15n;
    const offer = await client.swaps.createOffer(maker, {
      payment: { chain: "Base", asset: "Native" },
      price,
      amount: Amount.fromTess("100"),
      minFill: Amount.fromTess("1"),
      payout: Buffer.from(evmMaker.address.slice(2), "hex"),
    });
    assert.equal((await client.swaps.offer(offer))?.available.toString(), "100");

    // A taker without TESS locks the payment of 2 TESS.
    const { keypair: taker } = Keypair.generate();
    const amount = Amount.fromTess("2");
    const due = paymentDue(amount.planck, price);
    const { secret, hash } = newSecret();
    const { time, number: from } = await evm.head();
    const lock = await evm.lockPayment({ asset: "Native", recipient: evmMaker.address, amount: due, hash, timelock: time + 86_400, offer, taker: taker.address });

    // The maker finds it, checks it and fills.
    const [found] = (await collector.locks(from, (await collector.head()).number, evmMaker.address)).filter((l) => l.id === lock);
    assert.ok(found);
    assert.equal(found.offer, offer);
    assert.equal(found.taker, taker.address);
    assert.equal(collector.amountOf(found, { chain: "Base", asset: "Native" }), due);
    assert.equal(tessFor(found.amount, price), amount.planck);
    const trade = await client.swaps.fill(maker, offer, { taker: found.taker, amount, hash: found.hash, paymentLock: Buffer.from(lock.slice(2), "hex") });
    assert.equal(await client.swaps.tradeOfLock("Base", Buffer.from(lock.slice(2), "hex")), trade);

    // The taker claims without fee; the secret pays the maker.
    const claimed = await client.swaps.claim(trade, secret);
    assert.equal((await client.balance(taker.address)).free.planck, amount.planck);
    const events = await client.swaps.events(claimed.blockNumber);
    const revealed = events.find((e) => e.type === "Claimed" && e.trade === trade);
    assert.ok(revealed && revealed.type === "Claimed");
    const before = await collector.balance(evmMaker.address, "Native");
    await collector.claim(lock, revealed.secret);
    assert.equal((await collector.lockStatus(lock))?.state, "Claimed");
    assert.ok((await collector.balance(evmMaker.address, "Native")) > before);

    await client.swaps.cancelOffer(maker, offer);
  } finally {
    client.destroy();
  }
});

test("no payment is locked where the contract is missing", { skip: !evmUrl && "set EVM_URL", timeout: 60_000 }, async () => {
  const network: EvmNetwork = {
    chain: "Base",
    name: "Local EVM",
    chainId: 31_337,
    rpcUrl: evmUrl!,
    usdc: zeroAddress,
    explorer: "",
    nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
  };
  const account = mnemonicToAccount("test test test test test test test test test test test junk", { addressIndex: 4 });
  const evm = new EvmSwaps(network, { walletClient: createWalletClient({ account, transport: http(evmUrl) }) });
  const rpc = (method: string, params: unknown[]) => evm.public.request({ method, params } as never);
  const code = await evm.public.getCode({ address: SWAP_CONTRACT });
  const snapshot = await rpc("evm_snapshot", []);
  try {
    await rpc("anvil_setCode", [SWAP_CONTRACT, "0x"]);
    const lock = { asset: "Native" as const, recipient: account.address, amount: 1n, hash: new Uint8Array(32), timelock: 4_000_000_000, offer: 0, taker: Keypair.fromUri("//Alice").address };
    await assert.rejects(evm.lockPayment(lock), /not deployed on Local EVM/);
  } finally {
    await rpc("evm_revert", [snapshot]);
  }
  assert.equal(await evm.public.getCode({ address: SWAP_CONTRACT }), code);
});
