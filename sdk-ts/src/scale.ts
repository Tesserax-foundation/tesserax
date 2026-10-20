/** SCALE compact encoding of a length. */
export const compactLength = (n: number): Uint8Array => {
  if (n < 0 || !Number.isInteger(n)) throw new RangeError("invalid length");
  if (n < 1 << 6) return Uint8Array.of(n << 2);
  if (n < 1 << 14) {
    const v = (n << 2) | 1;
    return Uint8Array.of(v & 0xff, v >> 8);
  }
  if (n < 2 ** 30) {
    const v = n * 4 + 2;
    return Uint8Array.of(v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff, (v >>> 24) & 0xff);
  }
  throw new RangeError("length too large");
};
