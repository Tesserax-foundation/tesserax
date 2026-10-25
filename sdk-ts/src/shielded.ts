/**
 * Private transactions with the shielded pool (`docs/privacy.md`).
 *
 * A {@link ShieldedAccount} is the shielded side of an account: its key derives from the seed of
 * the account's ML-DSA key, so the recovery phrase restores it. It reads the chain block after
 * block from its last synchronization, and its state can be stored and restored. Proofs are
 * generated locally, by the WebAssembly build of the shielded wallet of the chain, in Node.js as
 * in browsers.
 *
 * ```ts
 * const alice = Keypair.fromUri(phrase);
 * const shielded = await ShieldedAccount.create(alice);
 * await shielded.sync(client);
 * await shielded.shield(client, alice, Amount.fromTess("10"));
 * await shielded.send(client, "zte...", Amount.fromTess("4"));
 * ```
 */

import { getDynamicBuilder, getLookupFn } from "@polkadot-api/metadata-builders";
import { decAnyMetadata, unifyMetadata } from "@polkadot-api/substrate-bindings";
import { filter, firstValueFrom } from "rxjs";
import init, {
  callData,
  isShieldedAddress as isAddress,
  ShieldedWallet,
  transactExtrinsic,
} from "../wasm/tesserax_shielded_wasm.js";
import { Amount } from "./amount.js";
import { TransactionFailed, type TesseraxClient, type TxOutcome } from "./client.js";
import { accountIdOf, parseAddress, type Keypair } from "./keys.js";

let loaded: Promise<unknown> | undefined;

/** Load the WebAssembly module of the shielded wallet, once. The other functions of this module
 * load it themselves; {@link isShieldedAddress} needs it loaded. */
export function loadShielded(): Promise<void> {
  loaded ??= (async () => {
    const wasm = new URL("../wasm/tesserax_shielded_wasm_bg.wasm", import.meta.url);
    if (typeof process !== "undefined" && process.versions?.node) {
      // Node.js cannot fetch a file URL: the module is read from the disk.
      const fs = "node:fs/promises";
      const { readFile } = (await import(/* @vite-ignore */ fs)) as typeof import("node:fs/promises");
      await init({ module_or_path: await readFile(wasm) });
    } else {
      await init({ module_or_path: wasm });
    }
  })().catch((error) => {
    loaded = undefined;
    throw error;
  });
  return loaded as Promise<void>;
}

/** Whether `text` is a shielded address, `zte...`, once {@link loadShielded} resolved. */
export function isShieldedAddress(text: string): boolean {
  return isAddress(text.trim());
}

const toHex = (bytes: Uint8Array) => "0x" + Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
const fromHex = (hex: string) => {
  const clean = hex.startsWith("0x") ? hex.slice(2) : hex;
  return Uint8Array.from({ length: clean.length / 2 }, (_, i) => parseInt(clean.slice(2 * i, 2 * i + 2), 16));
};
const concat = (...parts: Uint8Array[]) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let offset = 0;
  for (const p of parts) {
    out.set(p, offset);
    offset += p.length;
  }
  return out;
};

/** The 32 bytes of a digest as polkadot-api decodes it: four 64-bit limbs, little-endian. */
function digestBytes(value: unknown): Uint8Array {
  if (value instanceof Uint8Array) return value;
  if (typeof value === "string") return fromHex(value);
  const bytes = new Uint8Array(32);
  const view = new DataView(bytes.buffer);
  (value as Array<bigint | number>).forEach((limb, i) => view.setBigUint64(i * 8, BigInt(limb), true));
  return bytes;
}

const bytesOf = (value: unknown): Uint8Array =>
  value instanceof Uint8Array ? value : typeof value === "string" ? fromHex(value) : (value as { asBytes(): Uint8Array }).asBytes();

/** A `Shielded.Transacted` event. */
interface Transacted {
  position: bigint;
  nullifiers: Uint8Array;
  commitments: Uint8Array;
  notes: [Uint8Array, Uint8Array];
}

// The chain, read with the legacy RPC: it answers for any block an archive node keeps, unlike
// the queries of polkadot-api, limited to the blocks it follows.

/** What the shielded wallet needs of the metadata: the indices of the shielded calls, and the
 * storage of the events. */
interface Runtime {
  pallet: number;
  shield: number;
  transact: number;
  eventsKey: string;
  decodeEvents: (hex: string) => Array<{ event: { type: string; value: { type: string; value: any } } }>;
}

const runtimes = new WeakMap<TesseraxClient, Promise<Runtime>>();

async function request<T>(client: TesseraxClient, method: string, params: unknown[]): Promise<T> {
  await client.client.getChainSpecData();
  return client.client._request<T>(method, params);
}

function runtimeOf(client: TesseraxClient): Promise<Runtime> {
  let runtime = runtimes.get(client);
  if (!runtime) {
    runtime = (async () => {
      const metadata = unifyMetadata(decAnyMetadata(await request<string>(client, "state_getMetadata", [])));
      const lookup = getLookupFn(metadata);
      const pallet = metadata.pallets.find((p) => p.name === "Shielded");
      if (!pallet?.calls) throw new Error("the chain has no shielded pool");
      const calls = lookup(pallet.calls.type);
      if (calls.type !== "enum") throw new Error("unexpected metadata of the shielded pool");
      const events = getDynamicBuilder(lookup).buildStorage("System", "Events");
      return {
        pallet: pallet.index,
        shield: calls.value.shield.idx,
        transact: calls.value.transact.idx,
        eventsKey: events.keys.enc(),
        decodeEvents: (hex: string) => events.value.dec(hex),
      };
    })();
    runtime.catch(() => runtimes.delete(client));
    runtimes.set(client, runtime);
  }
  return runtime;
}

/** The `Shielded.Transacted` events of the block `hash`. */
async function eventsAt(client: TesseraxClient, hash: string): Promise<Transacted[]> {
  const runtime = await runtimeOf(client);
  const value = await request<string | null>(client, "state_getStorage", [runtime.eventsKey, hash]);
  if (!value) return [];
  return runtime
    .decodeEvents(value)
    .filter((r) => r.event.type === "Shielded" && r.event.value.type === "Transacted")
    .map(({ event }) => {
      const v = event.value.value;
      return {
        position: BigInt(v.position),
        nullifiers: concat(...v.nullifiers.map(digestBytes)),
        commitments: concat(...v.commitments.map(digestBytes)),
        notes: [bytesOf(v.notes[0]), bytesOf(v.notes[1])],
      };
    });
}

const blockHash = (client: TesseraxClient, number: number) => request<string | null>(client, "chain_getBlockHash", [number]);

async function bestNumber(client: TesseraxClient): Promise<number> {
  const header = await request<{ number: string }>(client, "chain_getHeader", []);
  return Number(header.number);
}

/** The smallest fee of a private transfer or unshield, paid from the shielded balance. */
export async function shieldedMinimumFee(client: TesseraxClient): Promise<Amount> {
  return new Amount(BigInt(await client.api.constants.Shielded.MinimumFee()));
}

/** Submit a private transaction without signer, its proof authorizing it, and wait until a block
 * includes it. */
async function submitUnsigned(client: TesseraxClient, extrinsic: Uint8Array): Promise<TxOutcome> {
  let included;
  try {
    included = await firstValueFrom(
      client.client.submitAndWatch(extrinsic).pipe(filter((e) => e.type === "inBestBlock" || e.type === "finalized")),
    );
  } catch (error) {
    throw new TransactionFailed(`transaction refused: ${(error as Error).message}`);
  }
  if (included.type !== "inBestBlock" && included.type !== "finalized") throw new TransactionFailed("transaction not included");
  if (!included.ok) throw new TransactionFailed("transaction failed", "Shielded");
  return {
    blockHash: included.block.hash,
    blockNumber: included.block.number,
    txHash: included.txHash,
    events: included.events as unknown as TxOutcome["events"],
  };
}

/** What a synchronization found. */
export interface Synced {
  /** Number of the last block read. */
  block: number;
  /** Value of the notes received. */
  received: Amount;
}

const STATE_VERSION = 1;

/** The shielded side of an account. */
export class ShieldedAccount {
  private constructor(
    private readonly seed: Uint8Array,
    private wallet: ShieldedWallet,
    /** Last block read, and its hash, to notice a reorganization. */
    private synced: { block: number; hash: Uint8Array } | null,
  ) {}

  /** The shielded account of `keypair`, not synchronized. */
  static async create(keypair: Keypair): Promise<ShieldedAccount> {
    await loadShielded();
    const seed = keypair.seed();
    return new ShieldedAccount(seed, new ShieldedWallet(seed), null);
  }

  /** The shielded account of `keypair` from a state of {@link ShieldedAccount.state}. */
  static async restore(keypair: Keypair, state: Uint8Array): Promise<ShieldedAccount> {
    await loadShielded();
    if (state.length < 38 || state[0] !== STATE_VERSION) throw new Error("not a state of a shielded account");
    const view = new DataView(state.buffer, state.byteOffset, state.byteLength);
    const synced = state[1] === 1 ? { block: view.getUint32(2, true), hash: state.slice(6, 38) } : null;
    const seed = keypair.seed();
    return new ShieldedAccount(seed, new ShieldedWallet(seed, state.slice(38)), synced);
  }

  /** The state to store: the note tree, the notes and the last block read. It holds no key,
   * but tells which notes are the account's: keep it private. */
  state(): Uint8Array {
    const header = new Uint8Array(38);
    header[0] = STATE_VERSION;
    if (this.synced) {
      header[1] = 1;
      new DataView(header.buffer).setUint32(2, this.synced.block, true);
      header.set(this.synced.hash, 6);
    }
    return concat(header, this.wallet.state());
  }

  /** Its shielded address, `zte...`. */
  get address(): string {
    return this.wallet.address();
  }

  /** Spendable balance. */
  get balance(): Amount {
    return new Amount(this.wallet.balance());
  }

  /** Balance spent by transactions not yet seen in a block. */
  get pending(): Amount {
    return new Amount(this.wallet.pending());
  }

  /** Number of the last block read, if any. */
  get block(): number | undefined {
    return this.synced?.block;
  }

  /** Read the blocks of the best chain since the last synchronization. After a reorganization
   * below the last block read, the account is read again from the genesis. */
  async sync(client: TesseraxClient, onProgress?: (block: number) => void): Promise<Synced> {
    if (this.synced) {
      const hash = await blockHash(client, this.synced.block);
      if (hash !== toHex(this.synced.hash)) {
        this.wallet = new ShieldedWallet(this.seed);
        this.synced = null;
      }
    }
    const best = await bestNumber(client);
    let received = 0n;
    for (let number = this.synced ? this.synced.block + 1 : 0; number <= best; number++) {
      const hash = await blockHash(client, number);
      if (!hash) break;
      for (const event of await eventsAt(client, hash)) {
        received += this.wallet.ingest(event.position, event.nullifiers, event.commitments, event.notes[0], event.notes[1]);
      }
      this.synced = { block: number, hash: fromHex(hash) };
      if (number % 100 === 0) onProgress?.(number);
    }
    return { block: this.synced?.block ?? 0, received: new Amount(received) };
  }

  /** Move `amount` of the transparent balance of `signer` into this shielded account. */
  async shield(client: TesseraxClient, signer: Keypair, amount: Amount): Promise<TxOutcome> {
    const { pallet, shield } = await runtimeOf(client);
    const transaction = this.wallet.shield(signer.accountId, amount.planck);
    const tx = await client.api.txFromCallData(callData(pallet, shield, transaction));
    return client.submit(tx, signer);
  }

  /** Send `amount` privately to the shielded address `to`. The fee, the minimum fee, is paid
   * from the shielded balance. */
  async send(client: TesseraxClient, to: string, amount: Amount): Promise<TxOutcome> {
    const fee = await shieldedMinimumFee(client);
    return this.submit(client, this.wallet.send(to.trim(), amount.planck, fee.planck));
  }

  /** Move `amount` out of the pool to the transparent account `to`. The fee, the minimum fee, is
   * paid from the shielded balance. */
  async unshield(client: TesseraxClient, to: string | Uint8Array, amount: Amount): Promise<TxOutcome> {
    const fee = await shieldedMinimumFee(client);
    const recipient = typeof to === "string" ? parseAddress(to) : accountIdOf(to);
    return this.submit(client, this.wallet.unshield(recipient, amount.planck, fee.planck));
  }

  /** Make spendable again the notes of transactions that no block included. */
  releasePending(): void {
    this.wallet.releasePending();
  }

  private async submit(client: TesseraxClient, transaction: Uint8Array): Promise<TxOutcome> {
    const { pallet, transact } = await runtimeOf(client);
    const outcome = await submitUnsigned(client, transactExtrinsic(pallet, transact, transaction));
    this.wallet.markPending(transaction);
    return outcome;
  }
}
