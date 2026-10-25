/* tslint:disable */
/* eslint-disable */
export const memory: WebAssembly.Memory;
export const __wbg_shieldedwallet_free: (a: number, b: number) => void;
export const callData: (a: number, b: number, c: number, d: number) => [number, number, number, number];
export const isShieldedAddress: (a: number, b: number) => number;
export const shieldedwallet_address: (a: number) => [number, number];
export const shieldedwallet_balance: (a: number) => bigint;
export const shieldedwallet_ingest: (a: number, b: bigint, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => [bigint, number, number];
export const shieldedwallet_markPending: (a: number, b: number, c: number) => [number, number];
export const shieldedwallet_new: (a: number, b: number, c: number, d: number) => [number, number, number];
export const shieldedwallet_pending: (a: number) => bigint;
export const shieldedwallet_position: (a: number) => bigint;
export const shieldedwallet_releasePending: (a: number) => void;
export const shieldedwallet_root: (a: number) => [number, number];
export const shieldedwallet_send: (a: number, b: number, c: number, d: bigint, e: bigint) => [number, number, number, number];
export const shieldedwallet_shield: (a: number, b: number, c: number, d: bigint) => [number, number, number, number];
export const shieldedwallet_state: (a: number) => [number, number];
export const shieldedwallet_unshield: (a: number, b: number, c: number, d: bigint, e: bigint) => [number, number, number, number];
export const transactExtrinsic: (a: number, b: number, c: number, d: number) => [number, number, number, number];
export const __wbindgen_exn_store: (a: number) => void;
export const __externref_table_alloc: () => number;
export const __wbindgen_externrefs: WebAssembly.Table;
export const __wbindgen_malloc: (a: number, b: number) => number;
export const __externref_table_dealloc: (a: number) => void;
export const __wbindgen_free: (a: number, b: number, c: number) => void;
export const __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
export const __wbindgen_start: () => void;
