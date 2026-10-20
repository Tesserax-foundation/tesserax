/** Formatting of amounts, hashes, durations and rates. Amounts are planck strings (10^-12 TESS). */

export const DECIMALS = 12;
const PLANCK = 10n ** BigInt(DECIMALS);
export const TARGET_BLOCK_SECONDS = 18;

const group = (digits: string) => digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");

/** `planck` as TESS, with at most `maxDecimals` decimals and no trailing zeros. */
export function formatTess(planck: string | bigint | null | undefined, maxDecimals = 4): string {
  if (planck == null || planck === "") return "—";
  const value = BigInt(planck);
  const negative = value < 0n;
  const abs = negative ? -value : value;
  const whole = abs / PLANCK;
  let fraction = (abs % PLANCK).toString().padStart(DECIMALS, "0");
  // Small amounts keep their significant digits.
  const decimals = whole === 0n && abs > 0n ? Math.max(maxDecimals, fraction.search(/[1-9]/) + 2) : maxDecimals;
  fraction = fraction.slice(0, Math.min(decimals, DECIMALS)).replace(/0+$/, "");
  return `${negative ? "-" : ""}${group(whole.toString())}${fraction ? "." + fraction : ""}`;
}

/** Every decimal of `planck`, for tooltips. */
export const exactTess = (planck: string | bigint) => formatTess(planck, DECIMALS);

/** `planck` as a JS number of TESS, for charts only. */
export const tessNumber = (planck: string | bigint | null | undefined) =>
  planck == null ? 0 : Number(BigInt(planck) / 10n ** 6n) / 1e6;

export function formatNumber(value: number | string | bigint | null | undefined): string {
  if (value == null) return "—";
  return group(BigInt(typeof value === "number" ? Math.round(value) : value).toString());
}

const units = ["", "K", "M", "G", "T", "P", "E", "Z", "Y"];

/** 1234567 → "1.23 M". */
export function formatCompact(value: number | string | bigint | null | undefined, suffix = ""): string {
  if (value == null) return "—";
  let n = typeof value === "number" ? value : Number(value);
  let unit = 0;
  while (Math.abs(n) >= 1000 && unit < units.length - 1) {
    n /= 1000;
    unit++;
  }
  const digits = n >= 100 || unit === 0 ? 0 : n >= 10 ? 1 : 2;
  return `${n.toFixed(digits)}${units[unit] || suffix ? " " : ""}${units[unit]}${suffix}`;
}

export const formatHashrate = (hashesPerSecond: number | string | null | undefined) =>
  hashesPerSecond == null ? "—" : formatCompact(Number(hashesPerSecond), "H/s");

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds)) return "—";
  if (seconds < 60) return `${seconds < 10 ? seconds.toFixed(1) : Math.round(seconds)} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} h ${minutes % 60 ? `${minutes % 60} min` : ""}`.trim();
  const days = Math.floor(hours / 24);
  if (days < 365) return `${days} d ${hours % 24 ? `${hours % 24} h` : ""}`.trim();
  const years = days / 365.25;
  return `${years.toFixed(years < 10 ? 1 : 0)} years`;
}

/** Duration of `blocks` blocks at the target block time. */
export const blocksDuration = (blocks: number) => formatDuration(blocks * TARGET_BLOCK_SECONDS);

export function timeAgo(date: string | Date | null | undefined, now = Date.now()): string {
  if (!date) return "—";
  const seconds = Math.max(0, Math.round((now - new Date(date).getTime()) / 1000));
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds} s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  return days < 30 ? `${days} d ago` : new Date(date).toLocaleDateString();
}

export const formatDate = (date: string | Date | null | undefined) =>
  date ? new Date(date).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "medium" }) : "—";

/** "0x1234…cdef". */
export function shortHash(hash: string | null | undefined, head = 6, tail = 4): string {
  if (!hash) return "—";
  return hash.length <= head + tail + 2 ? hash : `${hash.slice(0, head + 2)}…${hash.slice(-tail)}`;
}

export function shortAddress(address: string | null | undefined, head = 6, tail = 6): string {
  if (!address) return "—";
  return address.length <= head + tail + 1 ? address : `${address.slice(0, head)}…${address.slice(-tail)}`;
}

export const isAddress = (value: string) => /^te[1-9A-HJ-NP-Za-km-z]{44,48}$/.test(value.trim());
export const isHash = (value: string) => /^0x[0-9a-fA-F]{64}$/.test(value.trim());
export const isBlockNumber = (value: string) => /^#?\d{1,12}$/.test(value.trim());

/** The UTF-8 text of `hex` when it is printable, else null. */
export function hexText(hex: string): string | null {
  if (!/^0x([0-9a-fA-F]{2})+$/.test(hex)) return null;
  const bytes = new Uint8Array(hex.slice(2).match(/../g)!.map((b) => parseInt(b, 16)));
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return /^[\p{L}\p{N}\p{P}\p{S}\p{Zs}\n\t]+$/u.test(text) ? text : null;
  } catch {
    return null;
  }
}

/** "Balances.transfer_keep_alive" → "Balances · transfer keep alive". */
export const callLabel = (pallet: string, call: string) => `${pallet} · ${call.replace(/_/g, " ")}`;

/** "BudgetExceeded" → "Budget exceeded". */
export const humanize = (name: string) =>
  name
    .replace(/_/g, " ")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/^./, (c) => c.toUpperCase())
    .replace(/ ([A-Z])(?=[a-z])/g, (_, c: string) => " " + c.toLowerCase());
