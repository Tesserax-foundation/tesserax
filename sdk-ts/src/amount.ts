/** Amounts of TESS, with 12 decimals. */

export const DECIMALS = 12;
export const PLANCK_PER_TESS = 10n ** BigInt(DECIMALS);

/** An amount of TESS, held in planck. Printed as a decimal string of TESS, such as "1.5". */
export class Amount {
  readonly planck: bigint;

  constructor(planck: bigint) {
    if (typeof planck !== "bigint" || planck < 0n || planck >= 1n << 128n) {
      throw new RangeError(`${String(planck)} is not an amount in planck`);
    }
    this.planck = planck;
  }

  /** Parse an amount of TESS such as "1.5" or "0.25". Numbers are refused: use strings. */
  static fromTess(text: string): Amount {
    const match = /^(\d*)(?:\.(\d*))?$/.exec(String(text).trim());
    const [, whole = "", fraction = ""] = match ?? [];
    if (!match || (whole === "" && fraction === "") || fraction.length > DECIMALS) {
      throw new RangeError(`"${text}" is not an amount of TESS with at most ${DECIMALS} decimals`);
    }
    return new Amount(BigInt(whole || "0") * PLANCK_PER_TESS + BigInt(fraction.padEnd(DECIMALS, "0") || "0"));
  }

  toString(): string {
    const whole = this.planck / PLANCK_PER_TESS;
    const fraction = this.planck % PLANCK_PER_TESS;
    if (fraction === 0n) return whole.toString();
    return `${whole}.${fraction.toString().padStart(DECIMALS, "0").replace(/0+$/, "")}`;
  }

  toJSON(): string {
    return this.toString();
  }

  equals(other: Amount): boolean {
    return this.planck === other.planck;
  }
}
