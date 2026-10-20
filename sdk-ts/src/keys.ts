/**
 * ML-DSA-65 keys and Tesserax addresses.
 *
 * Keys follow the node and the Rust SDK exactly: the 32-byte seed of a 24-word phrase is the
 * Substrate mini secret (PBKDF2-HMAC-SHA512 of the phrase entropy), hard derivation junctions
 * (`//name`) hash the parent seed, and an account is the BLAKE2-256 hash of the public key.
 * Transactions are signed with the FIPS 204 context "tesserax", with hedged randomness.
 */

import { ml_dsa65 } from "@noble/post-quantum/ml-dsa.js";
import { blake2b } from "@noble/hashes/blake2.js";
import { pbkdf2 } from "@noble/hashes/pbkdf2.js";
import { sha512 } from "@noble/hashes/sha2.js";
import { randomBytes } from "@noble/hashes/utils.js";
import { entropyToMnemonic, mnemonicToEntropy } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english.js";
import { AccountId } from "@polkadot-api/substrate-bindings";
import { compactLength } from "./scale.js";

/** SS58 prefix of Tesserax addresses: they start with "te". */
export const SS58_PREFIX = 2261;
/** FIPS 204 context of transaction signatures. */
export const SIGNING_CONTEXT = new TextEncoder().encode("tesserax");
export const PUBLIC_KEY_SIZE = 1952;
export const SIGNATURE_SIZE = 3309;
/** Phrase of the development keys (`//Alice`, `//Bob`, ...). */
export const DEV_PHRASE = "bottom drive obey lake curtain smoke basket hold race lonely fit walk";

const accountCodec = AccountId(SS58_PREFIX);
const utf8 = new TextEncoder();

const blake2_256 = (data: Uint8Array) => blake2b(data, { dkLen: 32 });

const concat = (...parts: Uint8Array[]) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let offset = 0;
  for (const p of parts) {
    out.set(p, offset);
    offset += p.length;
  }
  return out;
};


export const toHex = (bytes: Uint8Array) =>
  "0x" + Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

export const fromHex = (hex: string): Uint8Array => {
  const clean = hex.startsWith("0x") ? hex.slice(2) : hex;
  if (clean.length % 2 !== 0 || /[^0-9a-fA-F]/.test(clean)) throw new Error(`invalid hexadecimal: ${hex}`);
  return Uint8Array.from(clean.match(/../g) ?? [], (b) => parseInt(b, 16));
};

/** Chain code of a junction, as Substrate computes it: SCALE encoded, padded or hashed. */
const chainCode = (junction: string) => {
  let encoded: Uint8Array;
  if (/^\d+$/.test(junction) && BigInt(junction) < 1n << 64n) {
    encoded = new Uint8Array(8);
    new DataView(encoded.buffer).setBigUint64(0, BigInt(junction), true);
  } else {
    const raw = utf8.encode(junction);
    encoded = concat(compactLength(raw.length), raw);
  }
  if (encoded.length > 32) return blake2_256(encoded);
  const code = new Uint8Array(32);
  code.set(encoded);
  return code;
};

const deriveHard = (seed: Uint8Array, junction: string) => {
  const tag = utf8.encode("MlDsaHDKD");
  return blake2_256(concat(compactLength(tag.length), tag, seed, chainCode(junction)));
};

/** Substrate mini secret of a BIP39 phrase. */
const miniSecret = (phrase: string, password: string) => {
  const entropy = mnemonicToEntropy(phrase.normalize("NFKD"), wordlist);
  return pbkdf2(sha512, entropy, utf8.encode(`mnemonic${password}`), { c: 2048, dkLen: 64 }).slice(0, 32);
};

/** Account of an ML-DSA-65 public key. */
export const accountIdOf = (publicKey: Uint8Array) => blake2_256(publicKey);

/** Tesserax address of a 32-byte account. */
export const formatAddress = (account: Uint8Array): string => accountCodec.dec(account);

/** 32-byte account of a Tesserax address, or of a `0x` hexadecimal account id. */
export const parseAddress = (text: string): Uint8Array => {
  const value = text.trim();
  let account: Uint8Array;
  try {
    account = value.startsWith("0x") ? fromHex(value) : accountCodec.enc(value);
  } catch (error) {
    throw new Error(`invalid address ${value}: ${(error as Error).message}`);
  }
  if (account.length !== 32) throw new Error(`invalid address ${value}: not a 32-byte account`);
  if (!value.startsWith("0x") && formatAddress(account) !== value) {
    throw new Error(`invalid address ${value}: not a Tesserax address`);
  }
  return account;
};

/** Verify an ML-DSA-65 signature. */
export const verify = (
  publicKey: Uint8Array,
  message: Uint8Array,
  signature: Uint8Array,
  context: Uint8Array = SIGNING_CONTEXT,
) => ml_dsa65.verify(signature, message, publicKey, { context });

/** An ML-DSA-65 key pair that signs transactions. */
export class Keypair {
  readonly publicKey: Uint8Array;
  readonly accountId: Uint8Array;
  readonly address: string;
  readonly #secretKey: Uint8Array;

  constructor(seed: Uint8Array) {
    if (seed.length !== 32) throw new Error("a seed is 32 bytes");
    const { publicKey, secretKey } = ml_dsa65.keygen(seed);
    this.publicKey = publicKey;
    this.#secretKey = secretKey;
    this.accountId = accountIdOf(publicKey);
    this.address = formatAddress(this.accountId);
  }

  /**
   * A key from a secret URI: a 24-word phrase with optional hard derivation (`phrase//agent`)
   * and password (`phrase///password`), a development key (`//Alice`), or a `0x` hexadecimal
   * 32-byte seed.
   */
  static fromUri(uri: string): Keypair {
    const match = /^([^/]*)((?:\/\/?[^/]+)*)(?:\/\/\/(.*))?$/.exec(uri.trim());
    if (!match) throw new Error("malformed secret URI");
    const secret = match[1].trim() || DEV_PHRASE;
    const password = match[3] ?? "";
    let seed: Uint8Array;
    if (secret.startsWith("0x")) {
      seed = fromHex(secret);
      if (seed.length !== 32) throw new Error("a seed is 32 bytes");
    } else {
      try {
        seed = miniSecret(secret, password);
      } catch {
        throw new Error("invalid phrase");
      }
    }
    for (const [, separator, junction] of match[2].matchAll(/(\/\/?)([^/]+)/g)) {
      if (separator === "/") throw new Error("ML-DSA keys only support hard derivation (//)");
      seed = deriveHard(seed, junction);
    }
    return new Keypair(seed);
  }

  /** A new random key and its 24-word phrase (256 bits of entropy). */
  static generate(): { keypair: Keypair; phrase: string } {
    const phrase = entropyToMnemonic(randomBytes(32), wordlist);
    return { keypair: Keypair.fromUri(phrase), phrase };
  }

  /** Hedged ML-DSA-65 signature of `message`. */
  sign(message: Uint8Array, context: Uint8Array = SIGNING_CONTEXT): Uint8Array {
    return ml_dsa65.sign(message, this.#secretKey, { context });
  }

  toString(): string {
    return `Keypair(${this.address})`;
  }
}

export { concat };
