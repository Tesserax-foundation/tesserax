/** Micropayments: payment channels, vouchers and the HTTP 402 protocol. See
 * `docs/micropayments.md`. */

import { base64 } from "@scure/base";
import { Amount } from "./amount.js";
import type { TesseraxClient, TxOutcome } from "./client.js";
import { TransactionFailed } from "./client.js";
import { concat, fromHex, Keypair, accountIdOf, formatAddress, parseAddress, toHex, verify } from "./keys.js";
import { StorageError, loadJson, saveJson } from "./store.js";

/** FIPS 204 context of voucher signatures. */
export const VOUCHER_CONTEXT = new TextEncoder().encode("tesserax-channel");
/** Response header of a service asking for payment, with status 402. */
export const PAYMENT_HEADER = "Tesserax-Payment";
/** Request header carrying a voucher. */
export const VOUCHER_HEADER = "Tesserax-Voucher";
/** Blocks before a channel closes under which a service stops accepting its vouchers. */
export const CLOSE_SAFETY_BLOCKS = 100;
/** Version of the channel state files. */
const STATE_VERSION = 1;

/** Refuse a state file of another kind, version or chain, or whose `field` differs from `value`,
 * for each `field: [value, what]` of `expected`. */
function checkState(
  state: any,
  kind: string,
  file: string,
  genesis: Uint8Array,
  expected: Record<string, [string | null, string]>,
): void {
  if (typeof state !== "object" || state === null || state.version !== STATE_VERSION || state.kind !== kind) {
    throw new StorageError(`${file} is not a ${kind} state file of version ${STATE_VERSION}`);
  }
  if (state.genesis !== toHex(genesis)) throw new StorageError(`${file} was written for another chain`);
  for (const [field, [value, what]] of Object.entries(expected)) {
    if ((state[field] ?? null) !== value) throw new StorageError(`${file} was written for another ${what}`);
  }
}

/** Run `task` after the previous tasks of `chain` settled; returns the result and the new chain. */
function serialize<T>(chain: Promise<unknown>, task: () => Promise<T>): [Promise<T>, Promise<unknown>] {
  const next = chain.then(task);
  return [next, next.catch(() => undefined)];
}


const littleEndian = (value: bigint, bytes: number) => {
  const out = new Uint8Array(bytes);
  for (let i = 0; i < bytes; i++) out[i] = Number((value >> BigInt(8 * i)) & 0xffn);
  return out;
};

/** The message a voucher signs: SCALE encoding of `(genesis_hash, channel, amount)`. */
export const voucherMessage = (genesis: Uint8Array, channel: bigint, amount: Amount) =>
  concat(genesis, littleEndian(channel, 8), littleEndian(amount.planck, 16));

/** A voucher: the payer owes `amount` in total on `channel`. */
export class Voucher {
  constructor(
    readonly channel: bigint,
    readonly amount: Amount,
    readonly signature: Uint8Array,
  ) {}

  static sign(key: Keypair, genesis: Uint8Array, channel: bigint, amount: Amount): Voucher {
    return new Voucher(channel, amount, key.sign(voucherMessage(genesis, channel, amount), VOUCHER_CONTEXT));
  }

  /** Whether the voucher is signed by the ML-DSA-87 public key `signer`. */
  verify(genesis: Uint8Array, signer: Uint8Array): boolean {
    try {
      return verify(signer, voucherMessage(genesis, this.channel, this.amount), this.signature, VOUCHER_CONTEXT);
    } catch {
      return false;
    }
  }

  /** Value of the `Tesserax-Voucher` header. */
  toHeader(): string {
    return JSON.stringify({ channel: Number(this.channel), amount: this.amount.toString(), signature: base64.encode(this.signature) });
  }

  toJSON(): { channel: number; amount: string; signature: string } {
    return JSON.parse(this.toHeader());
  }

  static fromHeader(value: string): Voucher {
    const v = JSON.parse(value);
    if (!Number.isSafeInteger(v.channel) || v.channel < 0 || typeof v.amount !== "string" || typeof v.signature !== "string") {
      throw new Error("malformed voucher");
    }
    return new Voucher(BigInt(v.channel), Amount.fromTess(v.amount), base64.decode(v.signature));
  }
}

/** What a service asks for, in the `Tesserax-Payment` header of a 402 response. */
export interface PaymentRequired {
  payee: string;
  price: Amount;
  /** Genesis hash of the chain, hexadecimal. */
  genesis: string;
  /** Cumulative amount already accepted on the channel of a rejected voucher. */
  accepted?: Amount;
  /** Why a voucher was rejected. */
  error?: string;
}

export const paymentRequiredHeader = (r: PaymentRequired) =>
  JSON.stringify({
    payee: r.payee,
    price: r.price.toString(),
    genesis: r.genesis,
    ...(r.accepted ? { accepted: r.accepted.toString() } : {}),
    ...(r.error ? { error: r.error } : {}),
  });

export const parsePaymentRequired = (value: string): PaymentRequired => {
  const r = JSON.parse(value);
  if (typeof r.payee !== "string" || typeof r.price !== "string" || typeof r.genesis !== "string") {
    throw new Error("malformed payment requirement");
  }
  return {
    payee: r.payee,
    price: Amount.fromTess(r.price),
    genesis: r.genesis,
    accepted: typeof r.accepted === "string" ? Amount.fromTess(r.accepted) : undefined,
    error: typeof r.error === "string" ? r.error : undefined,
  };
};

/** A payment channel. */
export interface ChannelInfo {
  id: bigint;
  payer: string;
  payee: string;
  /** Account of the key signing the vouchers. */
  signer: string;
  signerPublic: Uint8Array;
  deposit: Amount;
  claimed: Amount;
  /** Block from which the close can be finalized, if the payer requested it. */
  closesAt?: number;
}

const bytesOf = (value: unknown): Uint8Array => {
  if (value instanceof Uint8Array) return value;
  if (typeof value === "string") return fromHex(value);
  if (Array.isArray(value) && value.length === 1) return bytesOf(value[0]);
  throw new Error("unexpected byte encoding");
};

/** Channel calls and queries, on top of a client. */
export class Channels {
  constructor(readonly client: TesseraxClient) {}

  private get api() {
    return this.client.api;
  }

  async channel(id: bigint): Promise<ChannelInfo | undefined> {
    const c = await this.api.query.Channels.Channels.getValue(id, { at: "best" });
    return c ? this.info(id, c) : undefined;
  }

  /** Open channels paid by `payer`. */
  async channelsOf(payer: string): Promise<ChannelInfo[]> {
    const account = formatAddress(parseAddress(payer));
    const entries = await this.api.query.Channels.Channels.getEntries({ at: "best" });
    return entries
      .filter(({ value }: { value: { payer: string } }) => formatAddress(parseAddress(value.payer)) === account)
      .map(({ keyArgs, value }: { keyArgs: [bigint]; value: unknown }) => this.info(keyArgs[0], value));
  }

  private info(id: bigint, c: any): ChannelInfo {
    const signerPublic = bytesOf(c.signer);
    return {
      id: BigInt(id),
      payer: formatAddress(parseAddress(c.payer)),
      payee: formatAddress(parseAddress(c.payee)),
      signer: formatAddress(accountIdOf(signerPublic)),
      signerPublic,
      deposit: new Amount(c.deposit),
      claimed: new Amount(c.claimed),
      closesAt: c.closes_at,
    };
  }

  private openCall(payee: string, signer: Keypair, deposit: Amount) {
    return this.api.tx.Channels.open({
      payee: formatAddress(parseAddress(payee)),
      // Fixed-size byte arrays are hexadecimal strings for polkadot-api.
      signer: toHex(signer.publicKey),
      deposit: deposit.planck,
    });
  }

  private static opened(outcome: TxOutcome): bigint {
    const event = outcome.events.find((e) => e.type === "Channels" && e.value.type === "Opened");
    if (!event) throw new TransactionFailed("the channel was not opened");
    return BigInt((event.value.value as { id: bigint }).id);
  }

  /** Open a channel from `payer` to `payee`, whose vouchers the payer's key signs. */
  async open(payer: Keypair, payee: string, deposit: Amount): Promise<bigint> {
    return Channels.opened(await this.client.submit(this.openCall(payee, payer, deposit), payer));
  }

  /** Open a channel from `owner` to `payee` through the delegation of `agent`, whose key signs
   * the vouchers. */
  async agentOpen(agent: Keypair, owner: string, payee: string, deposit: Amount): Promise<bigint> {
    return Channels.opened(await this.client.agentExecute(agent, owner, this.openCall(payee, agent, deposit)));
  }

  /** Add `amount` to the deposit, as the payer, or as its agent when `owner` is given. */
  topUp(signer: Keypair, owner: string | undefined, id: bigint, amount: Amount): Promise<TxOutcome> {
    const call = this.api.tx.Channels.top_up({ id, amount: amount.planck });
    return owner ? this.client.agentExecute(signer, owner, call) : this.client.submit(call, signer);
  }

  requestClose(signer: Keypair, owner: string | undefined, id: bigint): Promise<TxOutcome> {
    const call = this.api.tx.Channels.request_close({ id });
    return owner ? this.client.agentExecute(signer, owner, call) : this.client.submit(call, signer);
  }

  finalizeClose(signer: Keypair, id: bigint): Promise<TxOutcome> {
    return this.client.submit(this.api.tx.Channels.finalize_close({ id }), signer);
  }

  /** Claim `voucher` as the payee of its channel. */
  claim(payee: Keypair, voucher: Voucher): Promise<TxOutcome> {
    const call = this.api.tx.Channels.claim({
      id: voucher.channel,
      amount: voucher.amount.planck,
      signature: toHex(voucher.signature),
    });
    return this.client.submit(call, payee);
  }

  /** Close channel `id` as its payee. */
  close(payee: Keypair, id: bigint): Promise<TxOutcome> {
    return this.client.submit(this.api.tx.Channels.close({ id }), payee);
  }
}

/** A channel the payer uses. */
export interface PayerChannel {
  id: bigint;
  deposit: Amount;
  /** Cumulative amount of the latest voucher signed. */
  spent: Amount;
}

/**
 * Pays services over payment channels, one per payee, opened and topped up on demand. With
 * `owner`, channels are opened for the owner through the agent's delegation.
 *
 * With a state file (`ChannelPayer.withFile`), every channel opened, every top-up and every
 * voucher is written to disk before the voucher is returned: after a restart the payer resumes
 * its channels instead of opening new ones, and never signs a voucher below one it already gave.
 * A state file must be used by one payer at a time.
 */
export class ChannelPayer {
  readonly channels = new Map<string, PayerChannel>();
  private readonly ops: Channels;
  private genesis?: Uint8Array;
  private queue: Promise<unknown> = Promise.resolve();
  private file?: string;

  constructor(
    readonly client: TesseraxClient,
    readonly key: Keypair,
    readonly options: { owner?: string; deposit: Amount; maxPrice: Amount },
  ) {
    this.ops = new Channels(client);
  }

  /** A payer keeping its channels in the state file `file`, created if missing. Refuses a file
   * written for another chain, key or owner. */
  static async withFile(
    client: TesseraxClient,
    key: Keypair,
    options: { owner?: string; deposit: Amount; maxPrice: Amount },
    file: string,
  ): Promise<ChannelPayer> {
    const payer = new ChannelPayer(client, key, options);
    payer.genesis = await client.genesisHash();
    const state = (await loadJson(file)) as any;
    if (state !== undefined) {
      checkState(state, "payer", file, payer.genesis, { signer: [key.address, "key"], owner: [payer.ownerAddress(), "owner"] });
      for (const [payee, c] of Object.entries<any>(state.channels)) {
        payer.channels.set(payee, { id: BigInt(c.id), deposit: Amount.fromTess(c.deposit), spent: Amount.fromTess(c.spent) });
      }
    }
    payer.file = file;
    return payer;
  }

  private ownerAddress(): string | null {
    return this.options.owner ? formatAddress(parseAddress(this.options.owner)) : null;
  }

  private async persist(): Promise<void> {
    if (!this.file) return;
    const channels: Record<string, unknown> = {};
    for (const [payee, c] of this.channels) {
      channels[payee] = { id: Number(c.id), deposit: c.deposit.toString(), spent: c.spent.toString() };
    }
    await saveJson(this.file, {
      version: STATE_VERSION,
      kind: "payer",
      genesis: toHex(this.genesis!),
      signer: this.key.address,
      owner: this.ownerAddress(),
      channels,
    });
  }

  /** A voucher paying `requirement`, opening or topping up the channel if needed. */
  pay(requirement: PaymentRequired, maxPrice = this.options.maxPrice): Promise<Voucher> {
    // One payment at a time: vouchers are cumulative.
    const [next, queue] = serialize(this.queue, () => this.payNow(requirement, maxPrice));
    this.queue = queue;
    return next;
  }

  private async payNow(requirement: PaymentRequired, maxPrice: Amount): Promise<Voucher> {
    this.genesis ??= await this.client.genesisHash();
    if (requirement.genesis.toLowerCase() !== toHex(this.genesis)) throw new Error("the service is on another chain");
    if (requirement.price.planck > maxPrice.planck) {
      throw new Error(`the service asks ${requirement.price} TESS, more than the maximum of ${maxPrice}`);
    }
    if (requirement.price.planck === 0n) throw new Error("the service asks a price of zero");
    const payee = formatAddress(parseAddress(requirement.payee));

    let channel = this.channels.get(payee);
    if (channel && requirement.accepted && requirement.accepted.planck > channel.spent.planck) {
      throw new Error(`the service says ${requirement.accepted} TESS were paid on channel ${channel.id}, but only ${channel.spent} were`);
    }
    const open = channel && (await this.ops.channel(channel.id));
    if (!channel || !open || open.closesAt !== undefined) {
      const deposit = this.options.deposit.planck > requirement.price.planck ? this.options.deposit : requirement.price;
      const id = this.options.owner
        ? await this.ops.agentOpen(this.key, this.options.owner, payee, deposit)
        : await this.ops.open(this.key, payee, deposit);
      channel = { id, deposit, spent: new Amount(0n) };
      this.channels.set(payee, channel);
      await this.persist();
    }

    const amount = new Amount(channel.spent.planck + requirement.price.planck);
    if (amount.planck > channel.deposit.planck) {
      const missing = amount.planck - channel.deposit.planck;
      const topUp = new Amount(this.options.deposit.planck > missing ? this.options.deposit.planck : missing);
      await this.ops.topUp(this.key, this.options.owner, channel.id, topUp);
      channel = { ...channel, deposit: new Amount(channel.deposit.planck + topUp.planck) };
      this.channels.set(payee, channel);
      await this.persist();
    }

    // Recorded before the voucher leaves the payer.
    this.channels.set(payee, { ...channel, spent: amount });
    try {
      await this.persist();
    } catch (error) {
      this.channels.set(payee, channel);
      throw error;
    }
    return Voucher.sign(this.key, this.genesis, channel.id, amount);
  }

  /** `fetch`, paying the service if it answers 402 with a `Tesserax-Payment` header. */
  async fetch(input: string | URL, init: RequestInit = {}, maxPrice = this.options.maxPrice): Promise<Response> {
    const response = await fetch(input, init);
    const header = response.headers.get(PAYMENT_HEADER);
    if (response.status !== 402 || !header) return response;
    const voucher = await this.pay(parsePaymentRequired(header), maxPrice);
    const headers = new Headers(init.headers);
    headers.set(VOUCHER_HEADER, voucher.toHeader());
    return fetch(input, { ...init, headers });
  }
}

/** Why a service refused a voucher. */
export interface Rejection {
  reason: string;
  accepted?: Amount;
}

interface ReceivedChannel {
  /** State of the channel on chain, read at `fetched`; unknown after a restart. */
  info?: ChannelInfo;
  fetched: number;
  accepted: Amount;
  unclaimed?: Voucher;
}

const REFRESH_MS = 60_000;

/**
 * Verifies the vouchers paying a service and claims them.
 *
 * With a state file (`ChannelReceiver.withFile`), the amount accepted on each channel and the
 * latest voucher not claimed yet are written to disk before a voucher is accepted: after a
 * restart the service still claims what it was paid, and refuses vouchers it already accepted.
 * A state file must be used by one receiver at a time.
 */
export class ChannelReceiver {
  private readonly channels = new Map<bigint, ReceivedChannel>();
  private readonly ops: Channels;
  private genesis?: Uint8Array;
  private queue: Promise<unknown> = Promise.resolve();
  private file?: string;

  constructor(
    readonly client: TesseraxClient,
    readonly key: Keypair,
  ) {
    this.ops = new Channels(client);
  }

  /** A receiver keeping its state in the file `file`, created if missing. Refuses a file written
   * for another chain or service. */
  static async withFile(client: TesseraxClient, key: Keypair, file: string): Promise<ChannelReceiver> {
    const receiver = new ChannelReceiver(client, key);
    receiver.genesis = await client.genesisHash();
    const state = (await loadJson(file)) as any;
    if (state !== undefined) {
      checkState(state, "receiver", file, receiver.genesis, { payee: [key.address, "service"] });
      for (const [id, c] of Object.entries<any>(state.channels)) {
        receiver.channels.set(BigInt(id), {
          fetched: 0,
          accepted: Amount.fromTess(c.accepted),
          unclaimed: c.unclaimed ? Voucher.fromHeader(JSON.stringify(c.unclaimed)) : undefined,
        });
      }
    }
    receiver.file = file;
    return receiver;
  }

  private async persist(): Promise<void> {
    if (!this.file) return;
    const channels: Record<string, unknown> = {};
    for (const [id, c] of this.channels) {
      channels[id.toString()] = { accepted: c.accepted.toString(), unclaimed: c.unclaimed?.toJSON() ?? null };
    }
    await saveJson(this.file, {
      version: STATE_VERSION,
      kind: "receiver",
      genesis: toHex(this.genesis!),
      payee: this.key.address,
      channels,
    });
  }

  /** Run `task` after the verifications and claims in progress: they share the channel state. */
  private serial<T>(task: () => Promise<T>): Promise<T> {
    const [next, queue] = serialize(this.queue, task);
    this.queue = queue;
    return next;
  }

  /** The `Tesserax-Payment` header asking for `price`, after `rejection` if any. */
  async paymentRequired(price: Amount, rejection?: Rejection): Promise<string> {
    this.genesis ??= await this.client.genesisHash();
    return paymentRequiredHeader({
      payee: this.key.address,
      price,
      genesis: toHex(this.genesis),
      accepted: rejection?.accepted,
      error: rejection?.reason,
    });
  }

  /** Check the `Tesserax-Voucher` header of a request paying `price`. On success the voucher is
   * recorded for claiming. */
  verify(header: string, price: Amount): Promise<{ ok: true; amount: Amount } | { ok: false; rejection: Rejection }> {
    return this.serial(() => this.verifyNow(header, price));
  }

  private async verifyNow(header: string, price: Amount): Promise<{ ok: true; amount: Amount } | { ok: false; rejection: Rejection }> {
    const reject = (reason: string, accepted?: Amount) => ({ ok: false as const, rejection: { reason, accepted } });
    this.genesis ??= await this.client.genesisHash();
    let voucher: Voucher;
    try {
      voucher = Voucher.fromHeader(header);
    } catch (error) {
      return reject((error as Error).message);
    }
    let channel = this.channels.get(voucher.channel);
    if (!channel?.info || Date.now() - channel.fetched > REFRESH_MS || voucher.amount.planck > channel.info.deposit.planck) {
      const info = await this.ops.channel(voucher.channel);
      if (!info) return reject("unknown channel");
      const accepted = channel && channel.accepted.planck > info.claimed.planck ? channel.accepted : info.claimed;
      channel = { info, fetched: Date.now(), accepted, unclaimed: channel?.unclaimed };
      this.channels.set(voucher.channel, channel);
    }
    const info = channel.info!;
    if (info.payee !== this.key.address) return reject("the channel pays another payee");
    if (info.closesAt !== undefined) {
      const now = await this.client.bestBlockNumber();
      if (info.closesAt - now < CLOSE_SAFETY_BLOCKS) return reject("the channel is closing", channel.accepted);
    }
    if (voucher.amount.planck > info.deposit.planck) return reject("the voucher exceeds the deposit", channel.accepted);
    if (voucher.amount.planck < channel.accepted.planck + price.planck) {
      return reject("the voucher does not pay the price", channel.accepted);
    }
    if (!voucher.verify(this.genesis, info.signerPublic)) return reject("invalid voucher signature", channel.accepted);

    // Recorded before the request is served.
    const previous = { accepted: channel.accepted, unclaimed: channel.unclaimed };
    channel.accepted = voucher.amount;
    channel.unclaimed = voucher;
    try {
      await this.persist();
    } catch {
      Object.assign(channel, previous);
      return reject("the service cannot record the voucher");
    }
    return { ok: true, amount: voucher.amount };
  }

  /** Claim the latest voucher of every channel. A voucher stays recorded until its claim
   * succeeds; vouchers of channels closed in the meantime, or already claimed, are dropped. */
  claimAll(): Promise<TxOutcome[]> {
    return this.serial(async () => {
      const outcomes: TxOutcome[] = [];
      const pending = [...this.channels.values()].flatMap((c) => (c.unclaimed ? [c.unclaimed] : []));
      for (const voucher of pending) {
        const info = await this.ops.channel(voucher.channel);
        if (info && info.claimed.planck < voucher.amount.planck) outcomes.push(await this.ops.claim(this.key, voucher));
        if (!info) {
          // Closed: nothing can be claimed any more, and its vouchers are refused.
          this.channels.delete(voucher.channel);
        } else {
          const channel = this.channels.get(voucher.channel);
          if (channel?.unclaimed && channel.unclaimed.amount.planck <= voucher.amount.planck) channel.unclaimed = undefined;
          if (channel?.info && channel.info.claimed.planck < voucher.amount.planck) {
            channel.info = { ...channel.info, claimed: voucher.amount };
          }
        }
        await this.persist();
      }
      return outcomes;
    });
  }

  /** Amount accepted and not claimed yet. */
  unclaimed(): Amount {
    let total = 0n;
    for (const c of this.channels.values()) {
      if (c.unclaimed) total += c.unclaimed.amount.planck - (c.info?.claimed.planck ?? 0n);
    }
    return new Amount(total);
  }
}
