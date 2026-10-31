/**
 * Atomic swaps of TESS against ETH, USDC and USDT on EVM chains (`docs/swaps.md`).
 *
 * A maker offers TESS; a taker locks its payment on the EVM chain under the SHA-256 hash of a
 * secret; the maker checks it and fills the offer, which locks the TESS for the taker under the
 * same hash; the taker claims them with the secret, without fee, and the secret pays the maker on
 * the EVM chain.
 *
 * A buyer bids for TESS; a seller sells into the bid, locking its TESS for the buyer under the hash
 * of its own secret; the buyer locks the payment for the seller under the same hash and records
 * it; the seller claims the payment with the secret, which then claims the TESS for the buyer,
 * without fee.
 *
 * `Swaps` is the Tesserax side, `EvmSwaps` the side of the EVM chains.
 */

import { sha256 } from "@noble/hashes/sha2.js";
import { getDynamicBuilder, getLookupFn } from "@polkadot-api/metadata-builders";
import { decAnyMetadata, getSs58AddressInfo, unifyMetadata } from "@polkadot-api/substrate-bindings";
import { Enum, type TxEvent } from "polkadot-api";
import { filter, lastValueFrom, map, take } from "rxjs";
import {
  createPublicClient,
  getAddress,
  http,
  parseAbi,
  parseEventLogs,
  zeroAddress,
  type Address,
  type Hex,
  type PublicClient,
  type WalletClient,
} from "viem";
import { Amount, PLANCK_PER_TESS } from "./amount.js";
import type { TesseraxClient, TxOutcome } from "./client.js";
import { chainError, TransactionFailed } from "./errors.js";
import { Keypair, formatAddress, fromHex, parseAddress, toHex } from "./keys.js";

/** The chain a payment is made on. */
export type SwapChain = "Bitcoin" | "Ethereum" | "Base" | "Arbitrum";

/** The asset of a payment: BTC on Bitcoin, ETH on the other chains, or a stablecoin. */
export type SwapAsset = "Native" | "Usdc" | "Usdt";

/** What an offer is paid in. */
export interface Payment {
  chain: SwapChain;
  asset: SwapAsset;
}

/** The EVM chains of swaps. */
export const EVM_CHAINS: readonly SwapChain[] = ["Ethereum", "Base", "Arbitrum"];

/** Decimals of the asset of `payment`: prices count in its smallest unit. */
export const paymentDecimals = (payment: Payment): number =>
  payment.chain === "Bitcoin" ? 8 : payment.asset === "Native" ? 18 : 6;

/** Symbol of the asset of `payment`. */
export const paymentSymbol = (payment: Payment): string =>
  payment.chain === "Bitcoin" ? "BTC" : payment.asset === "Native" ? "ETH" : payment.asset === "Usdc" ? "USDC" : "USDT";

/** The payment, in the smallest unit of its asset, of `amount` planck at `price` units per TESS,
 * rounded up, as the chain computes it. */
export const paymentDue = (amount: bigint, price: bigint): bigint => {
  const product = amount * price;
  return product / PLANCK_PER_TESS + (product % PLANCK_PER_TESS === 0n ? 0n : 1n);
};

/** The TESS, in planck, a payment of `payment` units buys at `price`: the most whose payment due
 * is at most `payment`. */
export const tessFor = (payment: bigint, price: bigint): bigint => (price === 0n ? 0n : (payment * PLANCK_PER_TESS) / price);

/** SHA-256 of a secret. */
export const hashOf = (secret: Uint8Array): Uint8Array => sha256(secret);

/** A new secret of 32 bytes and its SHA-256 hash. */
export const newSecret = (): { secret: Uint8Array; hash: Uint8Array } => {
  const secret = crypto.getRandomValues(new Uint8Array(32));
  return { secret, hash: hashOf(secret) };
};

/** An offer of TESS. */
export interface Offer {
  id: number;
  maker: string;
  payment: Payment;
  /** Smallest units of the asset for one TESS. */
  price: bigint;
  available: Amount;
  minFill: Amount;
  /** The EVM address the maker is paid to, or its public key on Bitcoin. */
  payout: Uint8Array;
  fills: number;
  completed: number;
}

/** TESS locked for a taker. */
export interface Trade {
  id: number;
  offer: number;
  maker: string;
  taker: string;
  amount: Amount;
  price: bigint;
  payment: Payment;
  hash: Uint8Array;
  /** The lock of the payment on its chain. */
  paymentLock: Uint8Array;
  /** Last block the taker may claim at. */
  expires: number;
}

/** A bid for TESS. */
export interface Bid {
  id: number;
  buyer: string;
  payment: Payment;
  /** Smallest units of the asset for one TESS. */
  price: bigint;
  /** TESS still wanted. */
  wanted: Amount;
  minSale: Amount;
  /** Sales made into the bid, and sales the buyer paid and received. */
  sales: number;
  completed: number;
}

/** TESS a seller locked for the buyer of a bid. */
export interface Sale {
  id: number;
  bid: number;
  seller: string;
  buyer: string;
  amount: Amount;
  price: bigint;
  payment: Payment;
  hash: Uint8Array;
  /** The EVM address the seller is paid to, or its public key on Bitcoin. */
  payout: Uint8Array;
  /** The lock of the payment on its chain, empty until the buyer records it. */
  paymentLock: Uint8Array;
  /** Last block the buyer may claim at. */
  expires: number;
}

/** What happened to trades and sales in a block. */
export type SwapEvent =
  | { type: "Filled"; trade: Trade }
  /** Claimed with `secret`, which releases the payment. */
  | { type: "Claimed"; trade: number; taker: string; amount: Amount; secret: Uint8Array }
  | { type: "Refunded"; trade: number; maker: string; amount: Amount }
  | { type: "Sold"; sale: Sale }
  | { type: "SalePaid"; sale: number; paymentLock: Uint8Array }
  /** Claimed for the buyer with `secret`, which the seller revealed by claiming its payment. */
  | { type: "SaleClaimed"; sale: number; buyer: string; amount: Amount; secret: Uint8Array }
  | { type: "SaleRefunded"; sale: number; seller: string; amount: Amount };

const bytes = (value: unknown): Uint8Array =>
  value instanceof Uint8Array ? value : typeof value === "string" ? fromHex(value) : (value as { asBytes(): Uint8Array }).asBytes();

/** An account as a Tesserax address, from a decoded value: an address of any SS58 format, or
 * hexadecimal. */
const account = (value: unknown) => {
  const text = value as string;
  if (text.startsWith("0x")) return formatAddress(fromHex(text));
  const info = getSs58AddressInfo(text);
  if (!info.isValid) throw new Error(`invalid address ${text}`);
  return formatAddress(info.publicKey);
};

const paymentOf = (p: { chain: { type: string }; asset: { type: string } }): Payment => ({
  chain: p.chain.type as SwapChain,
  asset: p.asset.type as SwapAsset,
});

const encodePayment = (p: Payment) => ({ chain: Enum(p.chain), asset: Enum(p.asset) });

const offerOf = (id: unknown, o: any): Offer => ({
  id: Number(id),
  maker: account(o.maker),
  payment: paymentOf(o.payment),
  price: BigInt(o.price),
  available: new Amount(BigInt(o.available)),
  minFill: new Amount(BigInt(o.min_fill)),
  payout: bytes(o.payout),
  fills: Number(o.fills),
  completed: Number(o.completed),
});

const tradeOf = (id: unknown, t: any): Trade => ({
  id: Number(id),
  offer: Number(t.offer),
  maker: account(t.maker),
  taker: account(t.taker),
  amount: new Amount(BigInt(t.amount)),
  price: BigInt(t.price),
  payment: paymentOf(t.payment),
  hash: bytes(t.hash),
  paymentLock: bytes(t.payment_lock),
  expires: Number(t.expires),
});

const bidOf = (id: unknown, b: any): Bid => ({
  id: Number(id),
  buyer: account(b.buyer),
  payment: paymentOf(b.payment),
  price: BigInt(b.price),
  wanted: new Amount(BigInt(b.wanted)),
  minSale: new Amount(BigInt(b.min_sale)),
  sales: Number(b.sales),
  completed: Number(b.completed),
});

const saleOf = (id: unknown, s: any): Sale => ({
  id: Number(id),
  bid: Number(s.bid),
  seller: account(s.seller),
  buyer: account(s.buyer),
  amount: new Amount(BigInt(s.amount)),
  price: BigInt(s.price),
  payment: paymentOf(s.payment),
  hash: bytes(s.hash),
  payout: bytes(s.payout),
  paymentLock: s.payment_lock === undefined ? new Uint8Array() : bytes(s.payment_lock),
  expires: Number(s.expires),
});

const bounded = (value: Uint8Array) => {
  if (value.length > 64) throw new RangeError("at most 64 bytes");
  return value;
};

const fixed32 = (value: Uint8Array) => {
  if (value.length !== 32) throw new RangeError("32 bytes expected");
  return toHex(value);
};

/** The Tesserax side of swaps: offers, trades, and their calls. */
export class Swaps {
  constructor(private readonly client: TesseraxClient) {}

  private get api() {
    return this.client.api;
  }

  /** Open offers. */
  async offers(): Promise<Offer[]> {
    const entries = (await this.api.query.Swaps.Offers.getEntries({ at: "best" })) as Array<{ keyArgs: [unknown]; value: any }>;
    return entries.map(({ keyArgs, value }) => offerOf(keyArgs[0], value)).sort((a, b) => a.id - b.id);
  }

  /** The offer `id`, if open. */
  async offer(id: number): Promise<Offer | undefined> {
    const value = await this.api.query.Swaps.Offers.getValue(BigInt(id), { at: "best" });
    return value ? offerOf(id, value) : undefined;
  }

  /** Trades waiting for their claim or their refund. */
  async trades(): Promise<Trade[]> {
    const entries = (await this.api.query.Swaps.Trades.getEntries({ at: "best" })) as Array<{ keyArgs: [unknown]; value: any }>;
    return entries.map(({ keyArgs, value }) => tradeOf(keyArgs[0], value)).sort((a, b) => a.id - b.id);
  }

  /** The trade `id`, if not claimed nor refunded yet. */
  async trade(id: number): Promise<Trade | undefined> {
    const value = await this.api.query.Swaps.Trades.getValue(BigInt(id), { at: "best" });
    return value ? tradeOf(id, value) : undefined;
  }

  /** The trade the payment lock `lock` on `chain` filled, if any. */
  async tradeOfLock(chain: SwapChain, lock: Uint8Array): Promise<number | undefined> {
    const id = await this.api.query.Swaps.FilledLocks.getValue(Enum(chain), bounded(lock), { at: "best" });
    return id === undefined ? undefined : Number(id);
  }

  /** Offer `amount` TESS at `price` units of `payment` for one TESS, paid to `payout`, in trades
   * of at least `minFill`. Returns the id of the offer. */
  async createOffer(
    maker: Keypair,
    offer: { payment: Payment; price: bigint; amount: Amount; minFill: Amount; payout: Uint8Array },
  ): Promise<number> {
    const tx = this.api.tx.Swaps.offer({
      payment: encodePayment(offer.payment),
      price: offer.price,
      amount: offer.amount.planck,
      min_fill: offer.minFill.planck,
      payout: bounded(offer.payout),
    });
    const outcome = await this.client.submit(tx, maker);
    const event = outcome.events.find((e) => e.type === "Swaps" && e.value.type === "Offered");
    if (!event) throw new TransactionFailed("no offer created");
    return Number((event.value.value as { offer: bigint }).offer);
  }

  /** Change the price and the smallest fill of offer `id`, adding `add` TESS and withdrawing
   * `remove`. */
  async updateOffer(maker: Keypair, id: number, update: { price: bigint; minFill: Amount; add?: Amount; remove?: Amount }): Promise<TxOutcome> {
    const tx = this.api.tx.Swaps.update_offer({
      id: BigInt(id),
      price: update.price,
      min_fill: update.minFill.planck,
      add: update.add?.planck ?? 0n,
      remove: update.remove?.planck ?? 0n,
    });
    return this.client.submit(tx, maker);
  }

  /** Withdraw offer `id`: what remains returns to its maker. */
  async cancelOffer(maker: Keypair, id: number): Promise<TxOutcome> {
    return this.client.submit(this.api.tx.Swaps.cancel_offer({ id: BigInt(id) }), maker);
  }

  /** Lock `amount` TESS of offer `id` for `taker` under `hash`, for the payment locked by
   * `paymentLock`, as the maker, once it checked the payment. Returns the id of the trade. */
  async fill(maker: Keypair, id: number, fill: { taker: string; amount: Amount; hash: Uint8Array; paymentLock: Uint8Array }): Promise<number> {
    const tx = this.api.tx.Swaps.fill({
      id: BigInt(id),
      taker: formatAddress(parseAddress(fill.taker)),
      amount: fill.amount.planck,
      hash: fixed32(fill.hash),
      payment_lock: bounded(fill.paymentLock),
    });
    const outcome = await this.client.submit(tx, maker);
    const event = outcome.events.find((e) => e.type === "Swaps" && e.value.type === "Filled");
    if (!event) throw new TransactionFailed("no trade filled");
    return Number((event.value.value as { trade: bigint }).trade);
  }

  /** Claim trade `id` for its taker with `secret`: submitted without signer and without fee, so
   * anyone can submit it, a taker without TESS first. */
  async claim(id: number, secret: Uint8Array): Promise<TxOutcome> {
    return this.submitWithSecret("claim", id, secret);
  }

  /** Open bids. */
  async bids(): Promise<Bid[]> {
    const entries = (await this.api.query.Swaps.Bids.getEntries({ at: "best" })) as Array<{ keyArgs: [unknown]; value: any }>;
    return entries.map(({ keyArgs, value }) => bidOf(keyArgs[0], value)).sort((a, b) => a.id - b.id);
  }

  /** The bid `id`, if open. */
  async bid(id: number): Promise<Bid | undefined> {
    const value = await this.api.query.Swaps.Bids.getValue(BigInt(id), { at: "best" });
    return value ? bidOf(id, value) : undefined;
  }

  /** Sales waiting for their claim or their refund. */
  async sales(): Promise<Sale[]> {
    const entries = (await this.api.query.Swaps.Sales.getEntries({ at: "best" })) as Array<{ keyArgs: [unknown]; value: any }>;
    return entries.map(({ keyArgs, value }) => saleOf(keyArgs[0], value)).sort((a, b) => a.id - b.id);
  }

  /** The sale `id`, if not claimed nor refunded yet. */
  async sale(id: number): Promise<Sale | undefined> {
    const value = await this.api.query.Swaps.Sales.getValue(BigInt(id), { at: "best" });
    return value ? saleOf(id, value) : undefined;
  }

  /** Bid for `amount` TESS at `price` units of `payment` for one TESS, in sales of at least
   * `minSale`. Only a deposit is held. Returns the id of the bid. */
  async createBid(buyer: Keypair, bid: { payment: Payment; price: bigint; amount: Amount; minSale: Amount }): Promise<number> {
    const tx = this.api.tx.Swaps.bid({
      payment: encodePayment(bid.payment),
      price: bid.price,
      amount: bid.amount.planck,
      min_sale: bid.minSale.planck,
    });
    const outcome = await this.client.submit(tx, buyer);
    const event = outcome.events.find((e) => e.type === "Swaps" && e.value.type === "BidPlaced");
    if (!event) throw new TransactionFailed("no bid placed");
    return Number((event.value.value as { bid: bigint }).bid);
  }

  /** Change the price of bid `id`, the TESS it still wants and its smallest sale. */
  async updateBid(buyer: Keypair, id: number, update: { price: bigint; wanted: Amount; minSale: Amount }): Promise<TxOutcome> {
    const tx = this.api.tx.Swaps.update_bid({ id: BigInt(id), price: update.price, wanted: update.wanted.planck, min_sale: update.minSale.planck });
    return this.client.submit(tx, buyer);
  }

  /** Withdraw bid `id`: its deposit returns to its buyer. */
  async cancelBid(buyer: Keypair, id: number): Promise<TxOutcome> {
    return this.client.submit(this.api.tx.Swaps.cancel_bid({ id: BigInt(id) }), buyer);
  }

  /** Sell `amount` TESS into bid `id`: they are locked for its buyer under `hash`, and the buyer
   * pays `payout`, an EVM address or a public key on Bitcoin. Returns the id of the sale. */
  async sell(seller: Keypair, id: number, sale: { amount: Amount; hash: Uint8Array; payout: Uint8Array }): Promise<number> {
    const tx = this.api.tx.Swaps.sell({ id: BigInt(id), amount: sale.amount.planck, hash: fixed32(sale.hash), payout: bounded(sale.payout) });
    const outcome = await this.client.submit(tx, seller);
    const event = outcome.events.find((e) => e.type === "Swaps" && e.value.type === "Sold");
    if (!event) throw new TransactionFailed("no sale made");
    return Number((event.value.value as { sale: bigint }).sale);
  }

  /** Record `paymentLock`, the lock of the payment of sale `id` on its chain, as its buyer. */
  async paySale(buyer: Keypair, id: number, paymentLock: Uint8Array): Promise<TxOutcome> {
    return this.client.submit(this.api.tx.Swaps.pay({ id: BigInt(id), payment_lock: bounded(paymentLock) }), buyer);
  }

  /** Claim sale `id` for its buyer with `secret`: submitted without signer and without fee, so
   * anyone can submit it, the seller once paid, or a buyer without TESS. */
  async claimSale(id: number, secret: Uint8Array): Promise<TxOutcome> {
    return this.submitWithSecret("claim_sale", id, secret);
  }

  /** Return sale `id` to its seller once it expired. Anyone can submit it. */
  async refundSale(signer: Keypair, id: number): Promise<TxOutcome> {
    return this.client.submit(this.api.tx.Swaps.refund_sale({ id: BigInt(id) }), signer);
  }

  /** `Swaps.<name>(id, secret)` as a general transaction without signer: the secret authorizes it,
   * and no fee is paid. */
  private async submitWithSecret(name: "claim" | "claim_sale", id: number, secret: Uint8Array): Promise<TxOutcome> {
    const call: Uint8Array = await this.api.tx.Swaps[name]({ id: BigInt(id), secret: fixed32(secret) }).getEncodedData();
    // Version 5, general transaction; extension version 0; era, nonce, tip, metadata hash mode.
    const body = new Uint8Array([0b0100_0000 | 5, 0, 0, 0, 0, 0, ...bytes(call)]);
    const transaction = new Uint8Array([...compact(body.length), ...body]);
    let included: Extract<TxEvent, { type: "inBestBlock" }>;
    try {
      included = await lastValueFrom(
        this.client.client.submitAndWatch(transaction).pipe(
          filter((e: TxEvent) => e.type === "inBestBlock" || e.type === "finalized"),
          take(1),
          map((e: TxEvent) => e as Extract<TxEvent, { type: "inBestBlock" }>),
        ),
      );
    } catch (error) {
      throw new TransactionFailed(`transaction refused: ${(error as Error).message}`);
    }
    if (!included.ok) {
      const error = chainError(included.dispatchError);
      throw new TransactionFailed(`transaction failed: ${error}`, error);
    }
    return {
      blockHash: included.block.hash,
      blockNumber: included.block.number,
      txHash: included.txHash,
      events: included.events as unknown as TxOutcome["events"],
    };
  }

  /** Return trade `id` to its maker once it expired. Anyone can submit it. */
  async refund(signer: Keypair, id: number): Promise<TxOutcome> {
    return this.client.submit(this.api.tx.Swaps.refund({ id: BigInt(id) }), signer);
  }

  private eventRecords(hash: string): Promise<EventRecord[]> {
    return eventRecordsAt(this.client, hash);
  }

  /** What happened to trades and sales in block `number` of the best chain. */
  async events(number: number): Promise<SwapEvent[]> {
    const hash = await this.client.contracts.blockHash(number);
    if (!hash) return [];
    const records = await this.eventRecords(hash);
    const found: SwapEvent[] = [];
    for (const { event } of records) {
      if (event.type !== "Swaps") continue;
      const e = event.value.value;
      if (event.value.type === "Filled") found.push({ type: "Filled", trade: tradeOf(e.trade, e) });
      else if (event.value.type === "Claimed")
        found.push({ type: "Claimed", trade: Number(e.trade), taker: account(e.taker), amount: new Amount(BigInt(e.amount)), secret: bytes(e.secret) });
      else if (event.value.type === "Refunded") found.push({ type: "Refunded", trade: Number(e.trade), maker: account(e.maker), amount: new Amount(BigInt(e.amount)) });
      else if (event.value.type === "Sold") found.push({ type: "Sold", sale: saleOf(e.sale, e) });
      else if (event.value.type === "SalePaid") found.push({ type: "SalePaid", sale: Number(e.sale), paymentLock: bytes(e.payment_lock) });
      else if (event.value.type === "SaleClaimed")
        found.push({ type: "SaleClaimed", sale: Number(e.sale), buyer: account(e.buyer), amount: new Amount(BigInt(e.amount)), secret: bytes(e.secret) });
      else if (event.value.type === "SaleRefunded")
        found.push({ type: "SaleRefunded", sale: Number(e.sale), seller: account(e.seller), amount: new Amount(BigInt(e.amount)) });
    }
    return found;
  }
}

type EventRecord = { event: { type: string; value: { type: string; value: any } } };

/** Decoders of the events of a chain, from its metadata, by client. */
const eventCodecs = new WeakMap<object, Promise<{ key: string; decode: (hex: string) => EventRecord[] }>>();

/** The events of the block `hash`. polkadot-api reads only the blocks it follows: the events of an
 * older block, which an archive node keeps, are read through the legacy RPC and decoded with the
 * metadata of the chain. */
async function eventRecordsAt(client: TesseraxClient, hash: string): Promise<EventRecord[]> {
  try {
    return (await client.api.query.System.Events.getValue({ at: hash })) as EventRecord[];
  } catch {
    let codec = eventCodecs.get(client);
    if (!codec) {
      codec = (async () => {
        const metadata = unifyMetadata(decAnyMetadata(await client.client._request<string>("state_getMetadata", [])));
        const events = getDynamicBuilder(getLookupFn(metadata)).buildStorage("System", "Events");
        return { key: events.keys.enc(), decode: (hex: string) => events.value.dec(hex) as EventRecord[] };
      })();
      eventCodecs.set(client, codec);
    }
    const { key, decode } = await codec;
    const raw = await client.client._request<string | null>("state_getStorage", [key, hash]);
    return raw ? decode(raw) : [];
  }
}

/** SCALE compact encoding of a length. */
const compact = (n: number): Uint8Array => {
  if (n < 1 << 6) return new Uint8Array([n << 2]);
  if (n < 1 << 14) return new Uint8Array([((n << 2) | 1) & 0xff, n >> 6]);
  if (n < 1 << 30) {
    const v = (n << 2) | 2;
    return new Uint8Array([v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff, (v >>> 24) & 0xff]);
  }
  throw new RangeError("length too large");
};

// The EVM side.

/** The swap contract, at the same address on every EVM chain. */
export const SWAP_CONTRACT: Address = "0xeA2f4f08D2A0f1972031B98De63f4B0395c256Fc";

/** ABI of the swap contract. */
export const SWAP_ABI = parseAbi([
  "function lock(address recipient, address token, uint256 amount, bytes32 hash, uint64 timelock, uint64 offer, bytes32 taker) payable returns (bytes32 id)",
  "function claim(bytes32 id, bytes32 secret)",
  "function refund(bytes32 id)",
  "function locks(bytes32 id) view returns (address sender, address recipient, address token, uint256 amount, bytes32 hash, uint64 timelock, uint8 state)",
  "event Locked(bytes32 indexed id, address indexed sender, address indexed recipient, address token, uint256 amount, bytes32 hash, uint64 timelock, uint64 offer, bytes32 taker)",
  "event Claimed(bytes32 indexed id, bytes32 secret)",
  "event Refunded(bytes32 indexed id)",
  "error InvalidLock()",
  "error WrongValue()",
  "error FeeOnTransfer()",
  "error NotLocked()",
  "error WrongSecret()",
  "error Expired()",
  "error NotExpired()",
  "error TransferFailed()",
]);

const ERC20_ABI = parseAbi([
  "function approve(address spender, uint256 amount) returns (bool)",
  "function allowance(address owner, address spender) view returns (uint256)",
  "function balanceOf(address owner) view returns (uint256)",
]);

/** An EVM chain with its stablecoins. */
export interface EvmNetwork {
  chain: SwapChain;
  name: string;
  chainId: number;
  /** A public RPC; users may give their own. */
  rpcUrl: string;
  usdc: Address;
  usdt?: Address;
  explorer: string;
  /** Currency of the chain, to add it to a browser wallet. */
  nativeCurrency: { name: string; symbol: string; decimals: number };
}

const ETHER = { name: "Ether", symbol: "ETH", decimals: 18 };

/** The EVM chains of swaps: the mainnets for the mainnet of Tesserax, their testnets for its
 * testnet. Token addresses are those of Circle for USDC and of Tether for USDT (USD₮0 on
 * Arbitrum); Base has no native USDT. */
export const evmNetworks = (testnet: boolean): EvmNetwork[] =>
  testnet
    ? [
        {
          chain: "Ethereum",
          name: "Ethereum Sepolia",
          chainId: 11_155_111,
          rpcUrl: "https://ethereum-sepolia-rpc.publicnode.com",
          usdc: "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
          explorer: "https://sepolia.etherscan.io",
          nativeCurrency: { name: "Sepolia Ether", symbol: "ETH", decimals: 18 },
        },
        {
          chain: "Base",
          name: "Base Sepolia",
          chainId: 84_532,
          rpcUrl: "https://sepolia.base.org",
          usdc: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
          explorer: "https://sepolia.basescan.org",
          nativeCurrency: { name: "Sepolia Ether", symbol: "ETH", decimals: 18 },
        },
        {
          chain: "Arbitrum",
          name: "Arbitrum Sepolia",
          chainId: 421_614,
          rpcUrl: "https://sepolia-rollup.arbitrum.io/rpc",
          usdc: "0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d",
          explorer: "https://sepolia.arbiscan.io",
          nativeCurrency: { name: "Sepolia Ether", symbol: "ETH", decimals: 18 },
        },
      ]
    : [
        {
          chain: "Ethereum",
          name: "Ethereum",
          chainId: 1,
          rpcUrl: "https://ethereum-rpc.publicnode.com",
          usdc: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
          usdt: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
          explorer: "https://etherscan.io",
          nativeCurrency: ETHER,
        },
        {
          chain: "Base",
          name: "Base",
          chainId: 8_453,
          rpcUrl: "https://mainnet.base.org",
          usdc: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
          explorer: "https://basescan.org",
          nativeCurrency: ETHER,
        },
        {
          chain: "Arbitrum",
          name: "Arbitrum",
          chainId: 42_161,
          rpcUrl: "https://arb1.arbitrum.io/rpc",
          usdc: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
          usdt: "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9",
          explorer: "https://arbiscan.io",
          nativeCurrency: ETHER,
        },
      ];

/** The token of `asset` on `network`, the zero address for ETH, if the chain has it. */
export const tokenOf = (network: EvmNetwork, asset: SwapAsset): Address | undefined =>
  asset === "Native" ? zeroAddress : asset === "Usdc" ? network.usdc : network.usdt;

/** The asset of `token` on `network`, if it is one of its assets. */
export const assetOf = (network: EvmNetwork, token: Address): SwapAsset | undefined =>
  (["Native", "Usdc", "Usdt"] as SwapAsset[]).find((a) => {
    const t = tokenOf(network, a);
    return t !== undefined && getAddress(t) === getAddress(token);
  });

/** State of a payment lock. */
export type LockState = "Locked" | "Claimed" | "Refunded";

const LOCK_STATES: Record<number, LockState> = { 1: "Locked", 2: "Claimed", 3: "Refunded" };

/** A payment lock as the contract stores it. */
export interface LockStatus {
  state: LockState;
  sender: Address;
  recipient: Address;
  token: Address;
  amount: bigint;
  hash: Uint8Array;
  /** Unix time from which the sender may take it back. */
  timelock: number;
}

/** A payment locked for a maker, from its event. */
export interface PaymentLock extends LockStatus {
  id: Hex;
  /** The offer it pays, and the Tesserax account of its taker. */
  offer: number;
  taker: string;
  /** Block of its event. */
  block: bigint;
}

/** Blocks `EvmSwaps.locks` reads in one request: public RPCs limit the range of logs. */
export const LOG_RANGE = 2_000n;

/** The EVM side of swaps on one chain: reads through a public client, sends through a wallet
 * client when given, such as a browser wallet. */
export class EvmSwaps {
  readonly public: PublicClient;
  readonly wallet?: WalletClient;

  constructor(
    readonly network: EvmNetwork,
    options: { rpcUrl?: string; publicClient?: PublicClient; walletClient?: WalletClient } = {},
  ) {
    this.public = options.publicClient ?? createPublicClient({ transport: http(options.rpcUrl ?? network.rpcUrl) });
    this.wallet = options.walletClient;
  }

  /** The latest block, and its unix time. */
  async head(): Promise<{ number: bigint; time: number }> {
    const block = await this.public.getBlock({ blockTag: "latest" });
    return { number: block.number, time: Number(block.timestamp) };
  }

  /** Lock `id` as the contract stores it, if it exists. */
  async lockStatus(id: Hex): Promise<LockStatus | undefined> {
    const [sender, recipient, token, amount, hash, timelock, state] = await this.public.readContract({
      address: SWAP_CONTRACT,
      abi: SWAP_ABI,
      functionName: "locks",
      args: [id],
    });
    const name = LOCK_STATES[state];
    if (!name) return undefined;
    return { state: name, sender, recipient, token, amount, hash: fromHex(hash), timelock: Number(timelock) };
  }

  /** Locks created in blocks `from` to `to` for `recipient`, if given, with their current state. */
  async locks(from: bigint, to: bigint, recipient?: Address): Promise<PaymentLock[]> {
    const found: PaymentLock[] = [];
    for (let start = from; start <= to; start += LOG_RANGE) {
      const end = start + LOG_RANGE - 1n < to ? start + LOG_RANGE - 1n : to;
      const logs = await this.public.getLogs({
        address: SWAP_CONTRACT,
        event: SWAP_ABI.find((item) => item.type === "event" && item.name === "Locked") as Extract<(typeof SWAP_ABI)[number], { type: "event"; name: "Locked" }>,
        args: recipient ? { recipient } : undefined,
        fromBlock: start,
        toBlock: end,
      });
      for (const log of logs) {
        const e = log.args;
        const status = await this.lockStatus(e.id!);
        found.push({
          id: e.id!,
          sender: e.sender!,
          recipient: e.recipient!,
          token: e.token!,
          amount: e.amount!,
          hash: fromHex(e.hash!),
          timelock: Number(e.timelock!),
          offer: Number(e.offer!),
          taker: formatAddress(fromHex(e.taker!)),
          state: status?.state ?? "Locked",
          block: log.blockNumber ?? 0n,
        });
      }
    }
    return found;
  }

  /** The amount of `asset` a lock pays, if it pays in this asset on this chain. */
  amountOf(lock: LockStatus, payment: Payment): bigint | undefined {
    return payment.chain === this.network.chain && assetOf(this.network, lock.token) === payment.asset ? lock.amount : undefined;
  }

  private async sender(): Promise<Address> {
    if (!this.wallet) throw new Error("no wallet to send with");
    const address = this.wallet.account?.address ?? (await this.wallet.getAddresses())[0];
    if (!address) throw new Error("the wallet gives no account");
    const chainId = await this.wallet.getChainId();
    if (chainId !== this.network.chainId) throw new Error(`the wallet is on chain ${chainId}, not on ${this.network.name}`);
    return address;
  }

  private async send(request: { functionName: string; args: readonly unknown[]; value?: bigint; address?: Address; abi?: unknown }): Promise<Hex> {
    const account = await this.sender();
    const hash = await this.wallet!.writeContract({
      address: request.address ?? SWAP_CONTRACT,
      abi: (request.abi ?? SWAP_ABI) as typeof SWAP_ABI,
      functionName: request.functionName as "lock",
      args: request.args as never,
      value: request.value,
      account,
      chain: null,
    } as Parameters<WalletClient["writeContract"]>[0]);
    const receipt = await this.public.waitForTransactionReceipt({ hash });
    if (receipt.status !== "success") throw new Error(`the transaction ${hash} reverted`);
    return hash;
  }

  /** Lock `amount` of `asset` for `recipient` until `timelock`, under `hash`, for `offer` and the
   * Tesserax account `taker`, approving the token first if needed. Returns the id of the lock. */
  async lockPayment(payment: {
    asset: SwapAsset;
    recipient: Address;
    amount: bigint;
    hash: Uint8Array;
    timelock: number;
    offer: number;
    taker: string;
  }): Promise<Hex> {
    const owner = await this.sender();
    // A payment sent to an address without the contract would be lost.
    if (!(await this.public.getCode({ address: SWAP_CONTRACT }))) throw new Error(`the swap contract is not deployed on ${this.network.name}`);
    const token = tokenOf(this.network, payment.asset);
    if (!token) throw new Error(`${payment.asset} is not on ${this.network.name}`);
    if (token !== zeroAddress) {
      const allowance = await this.public.readContract({ address: token, abi: ERC20_ABI, functionName: "allowance", args: [owner, SWAP_CONTRACT] });
      if (allowance < payment.amount) await this.send({ address: token, abi: ERC20_ABI, functionName: "approve", args: [SWAP_CONTRACT, payment.amount] });
    }
    const account = await this.sender();
    const hash = await this.wallet!.writeContract({
      address: SWAP_CONTRACT,
      abi: SWAP_ABI,
      functionName: "lock",
      args: [payment.recipient, token, payment.amount, toHex(payment.hash) as Hex, BigInt(payment.timelock), BigInt(payment.offer), toHex(parseAddress(payment.taker)) as Hex],
      value: token === zeroAddress ? payment.amount : 0n,
      account,
      chain: null,
    });
    const receipt = await this.public.waitForTransactionReceipt({ hash });
    if (receipt.status !== "success") throw new Error(`the lock ${hash} reverted`);
    const [locked] = parseEventLogs({ abi: SWAP_ABI, eventName: "Locked", logs: receipt.logs });
    if (!locked) throw new Error("no lock in the receipt");
    return locked.args.id;
  }

  /** Balance of `owner` in `asset`. */
  async balance(owner: Address, asset: SwapAsset): Promise<bigint> {
    const token = tokenOf(this.network, asset);
    if (!token) return 0n;
    if (token === zeroAddress) return this.public.getBalance({ address: owner });
    return this.public.readContract({ address: token, abi: ERC20_ABI, functionName: "balanceOf", args: [owner] });
  }

  /** Pay lock `id` to its recipient with `secret`. Returns the transaction hash. */
  async claim(id: Hex, secret: Uint8Array): Promise<Hex> {
    return this.send({ functionName: "claim", args: [id, toHex(secret)] });
  }

  /** The secret that claimed lock `id`, from its `Claimed` event in blocks `from` to the latest, if
   * it was claimed. */
  async secretOf(id: Hex, from: bigint): Promise<Uint8Array | undefined> {
    const event = SWAP_ABI.find((item) => item.type === "event" && item.name === "Claimed") as Extract<(typeof SWAP_ABI)[number], { type: "event"; name: "Claimed" }>;
    const head = await this.public.getBlockNumber();
    for (let start = from; start <= head; start += LOG_RANGE) {
      const end = start + LOG_RANGE - 1n < head ? start + LOG_RANGE - 1n : head;
      const logs = await this.public.getLogs({ address: SWAP_CONTRACT, event, args: { id }, fromBlock: start, toBlock: end });
      const found = logs.find((log) => log.args.id === id);
      if (found) return fromHex(found.args.secret!);
    }
    return undefined;
  }

  /** Return lock `id` to its sender once its timelock passed. Returns the transaction hash. */
  async refund(id: Hex): Promise<Hex> {
    return this.send({ functionName: "refund", args: [id] });
  }
}
