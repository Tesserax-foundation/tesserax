/** Micropayments: payment channels, vouchers and the HTTP 402 protocol. See
 * `docs/micropayments.md`. */

import { base64 } from "@scure/base";
import { Amount } from "./amount.js";
import type { TesseraxClient, TxOutcome } from "./client.js";
import { TransactionFailed } from "./client.js";
import { concat, fromHex, Keypair, accountIdOf, formatAddress, parseAddress, toHex, verify } from "./keys.js";

/** FIPS 204 context of voucher signatures. */
export const VOUCHER_CONTEXT = new TextEncoder().encode("tesserax-channel");
/** Response header of a service asking for payment, with status 402. */
export const PAYMENT_HEADER = "Tesserax-Payment";
/** Request header carrying a voucher. */
export const VOUCHER_HEADER = "Tesserax-Voucher";
/** Blocks before a channel closes under which a service stops accepting its vouchers. */
export const CLOSE_SAFETY_BLOCKS = 100;


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

  /** Whether the voucher is signed by the ML-DSA-65 public key `signer`. */
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
 */
export class ChannelPayer {
  readonly channels = new Map<string, PayerChannel>();
  private readonly ops: Channels;
  private genesis?: Uint8Array;
  private queue: Promise<unknown> = Promise.resolve();

  constructor(
    readonly client: TesseraxClient,
    readonly key: Keypair,
    readonly options: { owner?: string; deposit: Amount; maxPrice: Amount },
  ) {
    this.ops = new Channels(client);
  }

  /** A voucher paying `requirement`, opening or topping up the channel if needed. */
  pay(requirement: PaymentRequired, maxPrice = this.options.maxPrice): Promise<Voucher> {
    // One payment at a time: vouchers are cumulative.
    const next = this.queue.then(() => this.payNow(requirement, maxPrice));
    this.queue = next.catch(() => undefined);
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
    }

    const amount = new Amount(channel.spent.planck + requirement.price.planck);
    if (amount.planck > channel.deposit.planck) {
      const missing = amount.planck - channel.deposit.planck;
      const topUp = new Amount(this.options.deposit.planck > missing ? this.options.deposit.planck : missing);
      await this.ops.topUp(this.key, this.options.owner, channel.id, topUp);
      channel = { ...channel, deposit: new Amount(channel.deposit.planck + topUp.planck) };
    }
    const voucher = Voucher.sign(this.key, this.genesis, channel.id, amount);
    this.channels.set(payee, { ...channel, spent: amount });
    return voucher;
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
  info: ChannelInfo;
  fetched: number;
  accepted: Amount;
  unclaimed?: Voucher;
}

const REFRESH_MS = 60_000;

/** Verifies the vouchers paying a service and claims them. */
export class ChannelReceiver {
  private readonly channels = new Map<bigint, ReceivedChannel>();
  private readonly ops: Channels;
  private genesis?: Uint8Array;

  constructor(
    readonly client: TesseraxClient,
    readonly key: Keypair,
  ) {
    this.ops = new Channels(client);
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

  /** Check the `Tesserax-Voucher` header of a request paying `price`. */
  async verify(header: string, price: Amount): Promise<{ ok: true; amount: Amount } | { ok: false; rejection: Rejection }> {
    const reject = (reason: string, accepted?: Amount) => ({ ok: false as const, rejection: { reason, accepted } });
    this.genesis ??= await this.client.genesisHash();
    let voucher: Voucher;
    try {
      voucher = Voucher.fromHeader(header);
    } catch (error) {
      return reject((error as Error).message);
    }
    let channel = this.channels.get(voucher.channel);
    if (!channel || Date.now() - channel.fetched > REFRESH_MS || voucher.amount.planck > channel.info.deposit.planck) {
      const info = await this.ops.channel(voucher.channel);
      if (!info) return reject("unknown channel");
      const accepted = channel && channel.accepted.planck > info.claimed.planck ? channel.accepted : info.claimed;
      channel = { info, fetched: Date.now(), accepted, unclaimed: channel?.unclaimed };
      this.channels.set(voucher.channel, channel);
    }
    if (channel.info.payee !== this.key.address) return reject("the channel pays another payee");
    if (channel.info.closesAt !== undefined) {
      const now = await this.client.bestBlockNumber();
      if (channel.info.closesAt - now < CLOSE_SAFETY_BLOCKS) return reject("the channel is closing", channel.accepted);
    }
    if (voucher.amount.planck > channel.info.deposit.planck) return reject("the voucher exceeds the deposit", channel.accepted);
    if (voucher.amount.planck < channel.accepted.planck + price.planck) {
      return reject("the voucher does not pay the price", channel.accepted);
    }
    if (!voucher.verify(this.genesis, channel.info.signerPublic)) return reject("invalid voucher signature", channel.accepted);
    channel.accepted = voucher.amount;
    channel.unclaimed = voucher;
    return { ok: true, amount: voucher.amount };
  }

  /** Claim the latest voucher of every channel. */
  async claimAll(): Promise<TxOutcome[]> {
    const outcomes: TxOutcome[] = [];
    for (const channel of this.channels.values()) {
      const voucher = channel.unclaimed;
      if (!voucher) continue;
      channel.unclaimed = undefined;
      try {
        outcomes.push(await this.ops.claim(this.key, voucher));
      } catch (error) {
        // Keep the voucher for the next attempt, unless a newer one arrived meanwhile.
        const latest = channel.unclaimed as Voucher | undefined;
        if (!latest || latest.amount.planck < voucher.amount.planck) channel.unclaimed = voucher;
        throw error;
      }
    }
    return outcomes;
  }

  /** Amount accepted and not claimed yet. */
  unclaimed(): Amount {
    let total = 0n;
    for (const c of this.channels.values()) {
      if (c.unclaimed) total += c.unclaimed.amount.planck - c.info.claimed.planck;
    }
    return new Amount(total);
  }
}
