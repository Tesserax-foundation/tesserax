/**
 * The payment side of swaps on Bitcoin (`docs/swaps.md` of the chain): a P2WSH output locked by a
 * hash time-locked script, which pays the maker with the secret, or the taker once its timelock
 * passed.
 *
 * The taker locks the payment in one transaction: the script, `NOTICE_SATS` to the P2WPKH address
 * of the maker, which a maker watches, and an OP_RETURN output announcing the offer, the Tesserax
 * account of the taker, the hash, the timelock and the refund key. The refund key is the taker's
 * own, apart from the wallet paying: a browser wallet, such as the Bitcoin account of MetaMask,
 * pays (it signs only its own inputs), and the refund is signed with a key the Tesserax wallet
 * derives (`refundKey`), which no browser wallet could sign.
 */

import { secp256k1 } from "@noble/curves/secp256k1.js";
import { sha256 } from "@noble/hashes/sha2.js";
import * as btc from "@scure/btc-signer";
import { parseAddress } from "./keys.js";

/** Satoshis sent to the address of the maker to notify it, above the dust limit of P2WPKH. */
export const NOTICE_SATS = 330n;

/** Confirmations a maker waits for before filling: an unconfirmed transaction can be replaced. */
export const MIN_CONFIRMATIONS = 2;

/** Seconds a payment stays locked: 24 hours, compared to the median time of the last blocks. */
export const BITCOIN_LOCK_SECONDS = 24 * 3600;

export type BitcoinNetwork = "bitcoin" | "signet";

/** The Bitcoin network of a Tesserax network: Bitcoin for the mainnet, signet otherwise. */
export const bitcoinNetworkFor = (testnet: boolean): BitcoinNetwork => (testnet ? "signet" : "bitcoin");

const params = (network: BitcoinNetwork) => (network === "bitcoin" ? btc.NETWORK : btc.TEST_NETWORK);

/** A public Esplora API of `network`. */
export const defaultEsplora = (network: BitcoinNetwork) => (network === "bitcoin" ? "https://mempool.space/api" : "https://mempool.space/signet/api");

/** The parameters of a hash time-locked script; keys are compressed, 33 bytes. */
export interface Htlc {
  hash: Uint8Array;
  /** Key of the maker, which claims with the secret. */
  maker: Uint8Array;
  /** Key of the taker, which takes the payment back after `timelock`. */
  taker: Uint8Array;
  /** Unix time from which the taker may take it back. */
  timelock: number;
}

/** The script of `htlc`. */
export const htlcScript = (htlc: Htlc): Uint8Array =>
  btc.Script.encode([
    "IF",
    "SIZE",
    32,
    "EQUALVERIFY",
    "SHA256",
    htlc.hash,
    "EQUALVERIFY",
    htlc.maker,
    "ELSE",
    htlc.timelock,
    "CHECKLOCKTIMEVERIFY",
    "DROP",
    htlc.taker,
    "ENDIF",
    "CHECKSIG",
  ]);

/** The P2WSH output script of `htlc`. */
export const htlcOutputScript = (htlc: Htlc): Uint8Array => btc.OutScript.encode({ type: "wsh", hash: sha256(htlcScript(htlc)) });

/** The P2WPKH address of a compressed key: where a maker is notified and paid. */
export const addressOfKey = (key: Uint8Array, network: BitcoinNetwork): string => btc.p2wpkh(key, params(network)).address!;

/** What the OP_RETURN output of a payment announces to its maker, with the refund key (`TR`). */
export interface Announcement {
  offer: number;
  /** Tesserax account receiving the TESS. */
  taker: string;
  hash: Uint8Array;
  timelock: number;
  refund: Uint8Array;
}

const u64le = (n: number) => {
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt(n), true);
  return out;
};

const u32le = (n: number) => {
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, n, true);
  return out;
};

/** The 111 bytes of an announcement: `TR`, offer, taker, hash, timelock, refund key. */
export function encodeAnnouncement(a: Announcement): Uint8Array {
  if (a.hash.length !== 32 || a.refund.length !== 33) throw new RangeError("a hash of 32 bytes and a compressed key");
  return new Uint8Array([...new TextEncoder().encode("TR"), ...u64le(a.offer), ...parseAddress(a.taker), ...a.hash, ...u32le(a.timelock), ...a.refund]);
}

/** The key the Tesserax wallet takes the payment of hash `hash` back with: derived from the seed of
 * the account, so that the account recovers it. */
export function refundKey(seed: Uint8Array, hash: Uint8Array): { secretKey: Uint8Array; publicKey: Uint8Array } {
  const domain = new TextEncoder().encode("tesserax-swap-refund");
  let secretKey = sha256(new Uint8Array([...domain, ...seed, ...hash]));
  // A digest outside the scalars of the curve is hashed again: about one chance in 2^128.
  while (!secp256k1.utils.isValidSecretKey(secretKey)) secretKey = sha256(secretKey);
  return { secretKey, publicKey: secp256k1.getPublicKey(secretKey, true) };
}

/** An output of the wallet paying, with the transaction that created it. */
export interface PayingUtxo {
  txid: string;
  vout: number;
  amount: bigint;
  /** The raw transaction creating it: wallets check the amount against it. */
  rawTx: Uint8Array;
}

/** The transaction locking `amount` satoshis for the maker of `htlc`, to be signed by the wallet of
 * `payer`, a P2WPKH address, from its `utxos`, the change back to it. Returns the PSBT, the inputs
 * to sign, its fee, and the outpoint of the script. */
export function lockPsbt(options: {
  htlc: Htlc;
  announcement: Announcement;
  amount: bigint;
  payer: string;
  utxos: PayingUtxo[];
  feeRate: number;
  network: BitcoinNetwork;
}): { psbt: Uint8Array; inputs: number[]; fee: bigint; txid: string; vout: number } {
  const { htlc, announcement, amount, payer, network } = options;
  const net = params(network);
  const payerScript = btc.OutScript.encode(btc.Address(net).decode(payer));
  if (btc.OutScript.decode(payerScript).type !== "wpkh") throw new Error("the paying address must be a native SegWit (P2WPKH) address");
  const data = encodeAnnouncement(announcement);
  // Virtual size: version, counts and lock time; inputs of 68 vbytes; outputs of the script, the
  // notice, the announcement and the change.
  const vsize = (inputs: number) => 11 + 68 * inputs + 43 + 31 + (11 + data.length) + 31;
  const sorted = [...options.utxos].sort((a, b) => (a.amount > b.amount ? -1 : a.amount < b.amount ? 1 : 0));
  const chosen: PayingUtxo[] = [];
  let total = 0n;
  let fee = 0n;
  for (const utxo of sorted) {
    chosen.push(utxo);
    total += utxo.amount;
    fee = BigInt(Math.ceil(vsize(chosen.length) * options.feeRate));
    if (total >= amount + NOTICE_SATS + fee) break;
  }
  const change = total - amount - NOTICE_SATS - fee;
  if (change < 0n) throw new Error(`${total} satoshis available, ${amount + NOTICE_SATS + fee} needed with the fee`);

  const tx = new btc.Transaction({ allowUnknownOutputs: true });
  for (const utxo of chosen) {
    tx.addInput({
      txid: utxo.txid,
      index: utxo.vout,
      witnessUtxo: { script: payerScript, amount: utxo.amount },
      nonWitnessUtxo: utxo.rawTx,
      sequence: 0xfffffffd,
    });
  }
  tx.addOutput({ script: htlcOutputScript(htlc), amount });
  tx.addOutput({ script: btc.OutScript.encode({ type: "wpkh", hash: btc.p2wpkh(htlc.maker).hash }), amount: NOTICE_SATS });
  tx.addOutput({ script: btc.Script.encode(["RETURN", data]), amount: 0n });
  // Change below the dust limit goes to the fee.
  if (change >= 546n) tx.addOutput({ script: payerScript, amount: change });
  return { psbt: tx.toPSBT(0), inputs: chosen.map((_, i) => i), fee: change >= 546n ? fee : fee + change, txid: tx.id, vout: 0 };
}

/** The signed transaction of a PSBT the wallet signed: finalized if the wallet did not. */
export function extractSigned(psbt: Uint8Array): { raw: Uint8Array; txid: string } {
  const tx = btc.Transaction.fromPSBT(psbt, { allowUnknownOutputs: true });
  try {
    tx.finalize();
  } catch {
    // Already final.
  }
  return { raw: tx.extract(), txid: tx.id };
}

/** A payment locked by a script. */
export interface BitcoinLock {
  txid: string;
  vout: number;
  amount: bigint;
  htlc: Htlc;
}

/** The payment lock of a trade on Tesserax: the outpoint, the transaction id in its internal order
 * then the output index, little endian. */
export const outpointBytes = (txid: string, vout: number): Uint8Array => {
  const id = Uint8Array.from(txid.match(/../g)!.map((b) => Number.parseInt(b, 16))).reverse();
  return new Uint8Array([...id, ...u32le(vout)]);
};

/** The transaction taking `lock` back to `destination` with the refund key, once its timelock
 * passed. Returns it raw. */
export function refundTransaction(lock: BitcoinLock, secretKey: Uint8Array, destination: string, feeRate: number, network: BitcoinNetwork): Uint8Array {
  const script = htlcScript(lock.htlc);
  const destinationScript = btc.OutScript.encode(btc.Address(params(network)).decode(destination));
  // One P2WSH input of the refund branch and one output: about 160 virtual bytes.
  const fee = BigInt(Math.ceil(163 * feeRate));
  if (lock.amount - fee < 294n) throw new Error(`the fee takes the ${lock.amount} satoshis of the payment`);
  const tx = new btc.Transaction({ lockTime: lock.htlc.timelock, allowUnknownInputs: true, allowUnknownOutputs: true });
  tx.addInput({
    txid: lock.txid,
    index: lock.vout,
    witnessUtxo: { script: htlcOutputScript(lock.htlc), amount: lock.amount },
    witnessScript: script,
    sequence: 0xfffffffe,
  });
  tx.addOutput({ script: destinationScript, amount: lock.amount - fee });
  const digest = tx.preimageWitnessV0(0, script, btc.SigHash.ALL, lock.amount);
  const signature = secp256k1.sign(digest, secretKey, { prehash: false, format: "der", lowS: true });
  tx.updateInput(0, { finalScriptWitness: [new Uint8Array([...signature, btc.SigHash.ALL]), new Uint8Array(), script] }, true);
  return tx.extract();
}

/** A Bitcoin network through an Esplora API, such as the one of mempool.space. */
export class Esplora {
  readonly base: string;

  constructor(readonly network: BitcoinNetwork, url?: string) {
    this.base = (url ?? defaultEsplora(network)).replace(/\/$/, "");
  }

  private async get(path: string): Promise<Response> {
    for (let attempt = 1; ; attempt++) {
      const response = await fetch(this.base + path).catch((e: Error) => e);
      if (response instanceof Response && response.ok) return response;
      const retry = !(response instanceof Response) || response.status >= 500 || response.status === 429;
      if (!retry || attempt === 3) {
        const detail = response instanceof Response ? `${response.status} ${(await response.text()).slice(0, 200)}` : response.message;
        throw new Error(`${path}: ${detail}`);
      }
      await new Promise((r) => setTimeout(r, attempt * 1000));
    }
  }

  async height(): Promise<number> {
    return Number(await (await this.get("/blocks/tip/height")).text());
  }

  /** Median time of the last 11 blocks: the time a timelock compares to. */
  async medianTime(): Promise<number> {
    const hash = (await (await this.get("/blocks/tip/hash")).text()).trim();
    return Number(((await (await this.get(`/block/${hash}`)).json()) as { mediantime: number }).mediantime);
  }

  /** Fee rate, in satoshis per virtual byte, to confirm within `blocks` blocks. */
  async feeRate(blocks = 2): Promise<number> {
    const estimates = (await (await this.get("/fee-estimates")).json()) as Record<string, number>;
    const rates = Object.entries(estimates)
      .filter(([target]) => Number(target) <= Math.max(blocks, 1))
      .map(([, rate]) => rate);
    return Math.max(rates.length ? Math.min(...rates) : 2, 1);
  }

  async utxos(address: string): Promise<Array<{ txid: string; vout: number; amount: bigint; confirmed: boolean }>> {
    const found = (await (await this.get(`/address/${address}/utxo`)).json()) as Array<{ txid: string; vout: number; value: number; status: { confirmed: boolean } }>;
    return found.map((u) => ({ txid: u.txid, vout: u.vout, amount: BigInt(u.value), confirmed: u.status.confirmed }));
  }

  async rawTransaction(txid: string): Promise<Uint8Array> {
    return new Uint8Array(await (await this.get(`/tx/${txid}/raw`)).arrayBuffer());
  }

  /** Confirmations of `txid`: 0 in the mempool, undefined if unknown. */
  async confirmations(txid: string): Promise<number | undefined> {
    const response = await fetch(`${this.base}/tx/${txid}/status`);
    if (response.status === 404 || response.status === 400) return undefined;
    if (!response.ok) throw new Error(`/tx/${txid}/status: ${response.status}`);
    const status = (await response.json()) as { confirmed: boolean; block_height?: number };
    return status.confirmed && status.block_height !== undefined ? (await this.height()) - status.block_height + 1 : 0;
  }

  /** Whether the output `vout` of `txid` is spent. */
  async isSpent(txid: string, vout: number): Promise<boolean> {
    return ((await (await this.get(`/tx/${txid}/outspend/${vout}`)).json()) as { spent: boolean }).spent;
  }

  /** Broadcast a raw transaction. Returns its id. */
  async broadcast(raw: Uint8Array): Promise<string> {
    const hex = Array.from(raw, (b) => b.toString(16).padStart(2, "0")).join("");
    const response = await fetch(`${this.base}/tx`, { method: "POST", body: hex });
    const text = (await response.text()).trim();
    if (!response.ok) throw new Error(`broadcast refused: ${text}`);
    return text;
  }
}
