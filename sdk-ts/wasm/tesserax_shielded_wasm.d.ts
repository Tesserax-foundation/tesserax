/* tslint:disable */
/* eslint-disable */

/**
 * The shielded account of the account whose ML-DSA key has the seed `seed`.
 */
export class ShieldedWallet {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Its shielded address, `zte...`.
     */
    address(): string;
    /**
     * Spendable balance.
     */
    balance(): bigint;
    /**
     * Ingest the next `Shielded.Transacted` event. Returns the value it pays to this account.
     */
    ingest(position: bigint, nullifiers: Uint8Array, commitments: Uint8Array, note_0: Uint8Array, note_1: Uint8Array): bigint;
    /**
     * Mark the notes of a submitted transaction as spent until the chain shows it.
     */
    markPending(transaction_bytes: Uint8Array): void;
    /**
     * The shielded account of `seed`, restored from `state` if given.
     */
    constructor(seed: Uint8Array, state?: Uint8Array | null);
    /**
     * Balance spent by transactions not yet seen on chain.
     */
    pending(): bigint;
    /**
     * Commitments ingested: the position of the next one.
     */
    position(): bigint;
    /**
     * Make spendable again the notes of transactions that were not included.
     */
    releasePending(): void;
    /**
     * Root of the note tree, the 32 bytes of its SCALE encoding.
     */
    root(): Uint8Array;
    /**
     * Send `value` to the shielded address `to`, paying `fee` from the pool.
     */
    send(to: string, value: bigint, fee: bigint): Uint8Array;
    /**
     * Shield `value` of the transparent account `source`.
     */
    shield(source: Uint8Array, value: bigint): Uint8Array;
    /**
     * The tree and the notes, to store.
     */
    state(): Uint8Array;
    /**
     * Send `value` from the pool to the transparent account `recipient`, paying `fee`.
     */
    unshield(recipient: Uint8Array, value: bigint, fee: bigint): Uint8Array;
}

/**
 * Call data of `Shielded.<call>(transaction)`.
 */
export function callData(pallet: number, call: number, transaction_bytes: Uint8Array): Uint8Array;

/**
 * Whether `text` is a shielded address.
 */
export function isShieldedAddress(text: string): boolean;

/**
 * A `Shielded.transact` extrinsic without signer.
 */
export function transactExtrinsic(pallet: number, call: number, transaction_bytes: Uint8Array): Uint8Array;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_shieldedwallet_free: (a: number, b: number) => void;
    readonly callData: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly isShieldedAddress: (a: number, b: number) => number;
    readonly shieldedwallet_address: (a: number) => [number, number];
    readonly shieldedwallet_balance: (a: number) => bigint;
    readonly shieldedwallet_ingest: (a: number, b: bigint, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => [bigint, number, number];
    readonly shieldedwallet_markPending: (a: number, b: number, c: number) => [number, number];
    readonly shieldedwallet_new: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly shieldedwallet_pending: (a: number) => bigint;
    readonly shieldedwallet_position: (a: number) => bigint;
    readonly shieldedwallet_releasePending: (a: number) => void;
    readonly shieldedwallet_root: (a: number) => [number, number];
    readonly shieldedwallet_send: (a: number, b: number, c: number, d: bigint, e: bigint) => [number, number, number, number];
    readonly shieldedwallet_shield: (a: number, b: number, c: number, d: bigint) => [number, number, number, number];
    readonly shieldedwallet_state: (a: number) => [number, number];
    readonly shieldedwallet_unshield: (a: number, b: number, c: number, d: bigint, e: bigint) => [number, number, number, number];
    readonly transactExtrinsic: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
