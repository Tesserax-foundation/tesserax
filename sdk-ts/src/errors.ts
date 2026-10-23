/** Errors of transactions. */

/** The chain refused or failed a transaction. `error` is the chain error, such as
 * `Agents::BudgetExceeded`, when known. */
export class TransactionFailed extends Error {
  constructor(
    message: string,
    readonly error?: string,
  ) {
    super(message);
    this.name = "TransactionFailed";
  }
}

/** Name of a dispatch error, such as `Agents::BudgetExceeded`. */
export const chainError = (dispatchError: { type: string; value: unknown }) => {
  const inner = dispatchError.value as { type?: string; value?: { type?: string } } | undefined;
  if (dispatchError.type === "Module" && inner?.type && inner.value?.type) {
    return `${inner.type}::${inner.value.type}`;
  }
  return inner?.type ? `${dispatchError.type}::${inner.type}` : dispatchError.type;
};
