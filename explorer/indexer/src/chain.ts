/** Decoding of Tesserax blocks with the metadata of their runtime. */

import { blake2b } from "@noble/hashes/blake2.js";
import { getDynamicBuilder, getLookupFn } from "@polkadot-api/metadata-builders";
import {
  AccountId,
  compactNumber,
  decAnyMetadata,
  Tuple,
  unifyMetadata,
  type UnifiedMetadata,
} from "@polkadot-api/substrate-bindings";
import type { Rpc } from "./rpc.js";

/** Byte strings longer than this are stored as their length and hash, not their content
 * (shielded proofs are about 180 kB). */
const MAX_INLINE_BYTES = 512;

export const toHex = (bytes: Uint8Array) => "0x" + Buffer.from(bytes).toString("hex");
export const fromHex = (hex: string) => new Uint8Array(Buffer.from(hex.startsWith("0x") ? hex.slice(2) : hex, "hex"));
export const blake2_256 = (bytes: Uint8Array) => blake2b(bytes, { dkLen: 32 });

/** A decoded value made JSON friendly: integers as decimal strings, bytes as hex. */
export function toJson(value: unknown): unknown {
  if (value === undefined || value === null) return null;
  if (typeof value === "bigint") return value.toString();
  if (value instanceof Uint8Array) {
    return value.length > MAX_INLINE_BYTES ? { bytes: value.length, blake2: toHex(blake2_256(value)) } : toHex(value);
  }
  if (Array.isArray(value)) return value.map(toJson);
  if (typeof value === "object") {
    const object = value as Record<string, unknown>;
    // Enums decode as { type, value }.
    if (typeof object.type === "string" && Object.keys(object).length <= 2 && "value" in object) {
      return object.value === undefined ? object.type : { [object.type]: toJson(object.value) };
    }
    if (typeof object.type === "string" && Object.keys(object).length === 1) return object.type;
    return Object.fromEntries(Object.entries(object).map(([k, v]) => [k, toJson(v)]));
  }
  if (typeof value === "string" && value.startsWith("0x") && value.length > 2 + 2 * MAX_INLINE_BYTES) {
    return { bytes: (value.length - 2) / 2, blake2: toHex(blake2_256(fromHex(value))) };
  }
  return value;
}

/** A 256-bit integer decoded as its four little-endian 64-bit limbs. */
export const u256 = (limbs: unknown): bigint =>
  Array.isArray(limbs) ? limbs.reduce((acc: bigint, limb, i) => acc + (BigInt(limb as bigint) << BigInt(64 * i)), 0n) : BigInt(limbs as bigint);

export interface DecodedExtrinsic {
  index: number;
  hash: string;
  size: number;
  version: number;
  signer: string | null;
  nonce: number | null;
  tip: bigint | null;
  pallet: string;
  call: string;
  args: unknown;
  /** Raw decoded arguments, for the indexer's own use (Timestamp.set). */
  rawArgs: any;
}

export interface DecodedEvent {
  index: number;
  extrinsicIndex: number | null;
  pallet: string;
  name: string;
  /** JSON friendly data. */
  data: any;
}

/** Codecs of one runtime version. */
export class Runtime {
  readonly metadata: UnifiedMetadata;
  readonly builder: ReturnType<typeof getDynamicBuilder>;
  readonly ss58: number;
  private readonly address: (bytes: Uint8Array) => string;
  private readonly extrinsicCodecs: { address: any; signature: any; extra: any; call: any };
  private readonly events: ReturnType<ReturnType<typeof getDynamicBuilder>["buildStorage"]>;

  constructor(readonly specVersion: number, metadataHex: string) {
    this.metadata = unifyMetadata(decAnyMetadata(metadataHex));
    this.builder = getDynamicBuilder(getLookupFn(this.metadata));
    this.ss58 = Number(this.builder.buildConstant("System", "SS58Prefix").dec(this.constant("System", "SS58Prefix")));
    const accountId = AccountId(this.ss58);
    this.address = (bytes) => accountId.dec(bytes);

    const extrinsic = this.metadata.extrinsic as any;
    if (typeof extrinsic.type === "number") {
      // Metadata v14: UncheckedExtrinsic<Address, Call, Signature, Extra>.
      const params = this.metadata.lookup[extrinsic.type].params;
      const param = (name: string) => {
        const p = params.find((p) => p.name === name);
        if (p?.type == null) throw new Error(`the extrinsic type has no ${name} parameter`);
        return this.builder.buildDefinition(p.type);
      };
      this.extrinsicCodecs = { address: param("Address"), signature: param("Signature"), extra: param("Extra"), call: param("Call") };
    } else {
      // Metadata v15 and later: the types and the extensions are listed.
      const extensions = (extrinsic.extensionsByVersion?.[0] ?? Object.values(extrinsic.extensions ?? {})) as Array<{ type: number }>;
      this.extrinsicCodecs = {
        address: this.builder.buildDefinition(extrinsic.address),
        signature: this.builder.buildDefinition(extrinsic.signature),
        extra: Tuple(...extensions.map((e) => this.builder.buildDefinition(e.type))),
        call: this.builder.buildDefinition(extrinsic.call),
      };
    }
    this.events = this.builder.buildStorage("System", "Events");
  }

  private constant(pallet: string, name: string): string {
    const value = this.metadata.pallets.find((p) => p.name === pallet)?.constants.find((c) => c.name === name)?.value;
    if (value === undefined) throw new Error(`no constant ${pallet}.${name}`);
    return value;
  }

  /** A runtime constant, decoded. */
  constantValue(pallet: string, name: string): any {
    return this.builder.buildConstant(pallet, name).dec(this.constant(pallet, name));
  }

  /** SS58 address of a 32-byte account id. */
  addressOf(accountId: Uint8Array | string): string {
    return this.address(typeof accountId === "string" ? fromHex(accountId) : accountId);
  }

  storageKey(pallet: string, item: string, ...keys: unknown[]): string {
    return this.builder.buildStorage(pallet, item).keys.enc(...keys);
  }

  decodeStorage(pallet: string, item: string, value: string | null): any {
    const storage = this.builder.buildStorage(pallet, item);
    return value == null ? storage.fallback : storage.value.dec(value);
  }

  get eventsKey(): string {
    return this.events.keys.enc();
  }

  decodeEvents(value: string | null): DecodedEvent[] {
    if (value == null) return [];
    const records = this.events.value.dec(value) as Array<{ phase: { type: string; value?: number }; event: any }>;
    return records.map((record, index) => ({
      index,
      extrinsicIndex: record.phase.type === "ApplyExtrinsic" ? Number(record.phase.value) : null,
      pallet: record.event.type,
      name: record.event.value.type,
      data: toJson(record.event.value.value) ?? {},
    }));
  }

  /** Decode the extrinsic `hex` (with its length prefix), at position `index` in its block. */
  decodeExtrinsic(hex: string, index: number): DecodedExtrinsic {
    const bytes = fromHex(hex);
    const hash = toHex(blake2_256(bytes));
    const length = compactNumber.dec(bytes);
    let offset = bytes.length - length;
    const preamble = bytes[offset++];
    const version = preamble & 0x3f;
    const kind = preamble >> 6;
    // Decoders read their input from the start of its buffer: give them copies, not views.
    const rest = () => bytes.slice(offset);
    let signer: string | null = null;
    let nonce: number | null = null;
    let tip: bigint | null = null;

    const decodeAt = <T>(codec: any): T => {
      // Decoders read from the start of the slice; measure the bytes they used by re-encoding.
      const value = codec.dec(rest());
      offset += codec.enc(value).length;
      return value;
    };

    if (version === 4 && kind === 2) {
      const address = decodeAt<{ type: string; value: any }>(this.extrinsicCodecs.address);
      signer = address.type === "Id" ? (typeof address.value === "string" ? address.value : this.addressOf(address.value)) : null;
      decodeAt(this.extrinsicCodecs.signature);
      const extra = decodeAt<any>(this.extrinsicCodecs.extra);
      ({ nonce, tip } = extensions(extra));
    } else if (version === 5 && kind === 1) {
      offset++; // extension version
      const extra = decodeAt<any>(this.extrinsicCodecs.extra);
      ({ nonce, tip } = extensions(extra));
    }
    const call = this.extrinsicCodecs.call.dec(rest()) as { type: string; value: { type: string; value: unknown } };
    return {
      index,
      hash,
      size: bytes.length,
      version,
      signer,
      nonce,
      tip,
      pallet: call.type,
      call: call.value.type,
      args: toJson(call.value.value) ?? {},
      rawArgs: call.value.value,
    };
  }

  /** Name of a dispatch error, such as `Agents.BudgetExceeded`. */
  errorName(error: any): string {
    if (typeof error === "string") return error;
    const [kind, value] = Object.entries(error ?? {})[0] ?? ["Unknown", null];
    const nameOf = (v: unknown) => (typeof v === "string" ? v : Object.keys((v as object) ?? {})[0] ?? "");
    if (kind !== "Module") return value && (typeof value === "object" || typeof value === "string") ? `${kind}.${nameOf(value)}` : String(kind);
    // Decoded with the metadata: { Module: { Pallet: "Error" } }.
    if (value && typeof value === "object" && !("index" in value)) {
      const [pallet, inner] = Object.entries(value)[0] ?? ["Module", null];
      return `${pallet}.${nameOf(inner)}`;
    }
    // Raw: { Module: { index, error } }.
    const module = value as { index: number | string; error: string };
    const pallet = this.metadata.pallets.find((p) => p.index === Number(module.index));
    if (!pallet?.errors) return `Module ${module.index}`;
    const variants = this.metadata.lookup[pallet.errors.type].def;
    const errorIndex = fromHex(module.error)[0];
    const variant = variants.tag === "variant" ? variants.value.find((v) => v.index === errorIndex) : undefined;
    return `${pallet.name}.${variant?.name ?? errorIndex}`;
  }
}

/** Nonce and tip in the decoded transaction extensions. */
function extensions(extra: any): { nonce: number | null; tip: bigint | null } {
  let nonce: number | null = null;
  let tip: bigint | null = null;
  const visit = (value: any) => {
    if (!value || typeof value !== "object") return;
    for (const [key, inner] of Object.entries(value)) {
      if (key === "nonce" || key === "CheckNonce") nonce = Number(inner as number);
      else if (key === "tip") tip = BigInt(inner as bigint);
    }
  };
  if (Array.isArray(extra)) {
    // A tuple of extensions, in the order of the metadata.
    for (const item of extra) {
      if (typeof item === "number" || typeof item === "bigint") nonce ??= Number(item);
      else if (item && typeof item === "object" && "tip" in item) tip = BigInt(item.tip);
      else visit(item);
    }
  } else visit(extra);
  return { nonce, tip };
}

/** Block header fields read from the digest. */
export interface PowDigest {
  author: string | null;
  nonce: string | null;
  powHash: string | null;
  runtimeUpdated: boolean;
}

const POW_ENGINE = "pow_";

/** Author (pre-runtime digest) and seal (nonce and RandomTESS hash) of a proof-of-work header. */
export function powDigest(logs: string[], runtime: Runtime): PowDigest {
  const digest: PowDigest = { author: null, nonce: null, powHash: null, runtimeUpdated: false };
  for (const log of logs) {
    const bytes = fromHex(log);
    const kind = bytes[0];
    if (kind === 8) {
      digest.runtimeUpdated = true;
      continue;
    }
    if (kind !== 5 && kind !== 6) continue;
    const engine = Buffer.from(bytes.subarray(1, 5)).toString("latin1");
    if (engine !== POW_ENGINE) continue;
    const payloadWithLength = bytes.slice(5);
    const length = compactNumber.dec(payloadWithLength);
    const payload = payloadWithLength.slice(payloadWithLength.length - length);
    if (kind === 6 && payload.length === 32) digest.author = runtime.addressOf(payload);
    if (kind === 5 && payload.length === 43) {
      digest.nonce = toHex(payload.slice(0, 11));
      digest.powHash = toHex(payload.slice(11));
    }
  }
  return digest;
}

/** The runtimes of the chain, by spec version, loaded on demand. */
export class Runtimes {
  private readonly bySpec = new Map<number, Runtime>();

  constructor(private readonly rpc: Rpc) {}

  /** The runtime that executed the blocks built on `parentHash`. */
  async at(parentHash: string): Promise<Runtime> {
    const version = await this.rpc.request<{ specVersion: number }>("state_getRuntimeVersion", [parentHash]);
    let runtime = this.bySpec.get(version.specVersion);
    if (!runtime) {
      const metadata = await this.rpc.request<string>("state_getMetadata", [parentHash]);
      runtime = new Runtime(version.specVersion, metadata);
      this.bySpec.set(version.specVersion, runtime);
    }
    return runtime;
  }
}
