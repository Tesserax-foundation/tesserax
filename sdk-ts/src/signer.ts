/**
 * Signing of Tesserax transactions with ML-DSA-87, as a `polkadot-api` transaction creator.
 *
 * A Tesserax transaction (extrinsic v4) is signed by an account, the BLAKE2-256 hash of an ML-DSA
 * public key, and carries the public key with the signature. The payload is the call, then the
 * explicit and implicit data of the transaction extensions in metadata order, hashed with
 * BLAKE2-256 when longer than 256 bytes.
 */

import { getLookupFn } from "@polkadot-api/metadata-builders";
import { withCommonExtensions, withNonce } from "@polkadot-api/signers-common";
import { decAnyMetadata, unifyMetadata } from "@polkadot-api/substrate-bindings";
import type { TxCreator } from "@polkadot-api/tx-creator";
import { firstValueFrom } from "rxjs";
import { compactLength } from "./scale.js";
import { concat, fromHex, Keypair, SIGNATURE_SIZE, toHex } from "./keys.js";

/** Signed extrinsic v4 marker. */
const SIGNED_V4 = 0x84;
/** `MultiAddress::Id`. */
const ADDRESS_ID = 0x00;

/** Whether the type `id` encodes to nothing. */
const isEmpty = (lookup: ReturnType<typeof getLookupFn>, id: number): boolean => {
  const entry = lookup(id);
  switch (entry.type) {
    case "void":
      return true;
    case "tuple":
      return entry.value.every((e) => isEmpty(lookup, e.id));
    case "struct":
      return Object.values(entry.value).every((e) => isEmpty(lookup, e.id));
    case "array":
      return entry.len === 0 || isEmpty(lookup, entry.value.id);
    default:
      return false;
  }
};

/** A transaction creator signing with `keypair`, with nonce and common extensions filled in. */
export const mlDsaTxCreator = (keypair: Keypair) => {
  const creator: TxCreator = async (payload, _options, bindings, mocked) => {
    const metadata = unifyMetadata(decAnyMetadata(payload.context.metadata));
    const lookup = getLookupFn(metadata);
    if ((payload.txExtVersion ?? 0) !== 0) throw new Error("only extrinsic v4 transactions are supported");

    const extra: Uint8Array[] = [];
    const additionalSigned: Uint8Array[] = [];
    for (const extension of metadata.extrinsic.extensionsByVersion[0]) {
      const supplied = payload.extensions.find(({ id }) => id === extension.identifier);
      if (supplied) {
        extra.push(fromHex(supplied.extra));
        additionalSigned.push(fromHex(supplied.additionalSigned));
      } else if (!isEmpty(lookup, extension.type) || !isEmpty(lookup, extension.additionalSigned)) {
        throw new Error(`missing data of the ${extension.identifier} transaction extension`);
      }
    }

    const callData = fromHex(payload.callData);
    const toSign = concat(callData, ...extra, ...additionalSigned);
    const message = toSign.length > 256 ? await firstValueFrom(bindings.hasher(toSign)) : toSign;
    const signature = mocked ? new Uint8Array(SIGNATURE_SIZE) : keypair.sign(message);

    const body = concat(
      Uint8Array.of(SIGNED_V4, ADDRESS_ID),
      keypair.accountId,
      keypair.publicKey,
      signature,
      ...extra,
      callData,
    );
    return toHex(concat(compactLength(body.length), body));
  };
  return withNonce(keypair.accountId)(withCommonExtensions(creator));
};
