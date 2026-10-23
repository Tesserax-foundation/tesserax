/**
 * Smart contracts: Solidity compiled to PolkaVM with `resolc`, deployed and called with ML-DSA
 * signed transactions, their arguments, results and events encoded with the Solidity ABI.
 *
 * ```ts
 * const token = await client.deployContract(alice, { code, abi, args: ["Tesserax Coin", 1_000_000n] });
 * await token.write(alice, "transfer", [toH160(bob.address), 10n]);
 * const balance = await token.read("balanceOf", [toH160(bob.address)]);
 * ```
 *
 * Reads are dry runs: the node executes the call on the state of the best block and returns its
 * result, without a transaction or a fee. Writes are dry run first, which sizes their weight and
 * storage deposit and stops a call that would revert before it costs a fee.
 */

import { keccak_256 } from "@noble/hashes/sha3.js";
import { Enum } from "polkadot-api";
import {
  decodeErrorResult,
  decodeEventLog,
  decodeFunctionResult,
  encodeAbiParameters,
  encodeFunctionData,
  getAddress,
  type Abi,
  type Hex,
} from "viem";
import { Amount } from "./amount.js";

type AbiConstructor = Extract<Abi[number], { type: "constructor" }>;
import type { TesseraxClient, TxOutcome } from "./client.js";
import { chainError, TransactionFailed } from "./errors.js";
import { formatAddress, fromHex, Keypair, parseAddress, toHex } from "./keys.js";

/** Weight and storage deposit of a transaction are those of its dry run, plus this margin in
 * percent: the state may change between the dry run and the block that includes it. */
const MARGIN_PERCENT = 10n;

/** Blocks `Contract.events` reads at most at once. */
export const MAX_EVENT_RANGE = 10_000;

/** The zero account, the caller of reads that do not name one. */
const ZERO_ACCOUNT = formatAddress(new Uint8Array(32));

/** A contract reverted. `reason` is decoded from its revert data, such as
 * `InsufficientBalance(available: 5, required: 10)`. */
export class ContractReverted extends TransactionFailed {
  constructor(
    readonly reason: string,
    readonly data: Uint8Array,
  ) {
    super(`contract reverted: ${reason}`, "Revive::ContractReverted");
    this.name = "ContractReverted";
  }
}

/** Address of an account in contracts: its `msg.sender`, and what `address` arguments name. The
 * last 20 bytes of the Keccak-256 hash of the account id, as the runtime maps it. */
export const toH160 = (address: string | Uint8Array): Hex => {
  const account = typeof address === "string" ? parseAddress(address) : address;
  // Accounts derived from an Ethereum address end with twelve 0xEE bytes: they map back to it.
  const raw = account.subarray(20).every((b) => b === 0xee) ? account.subarray(0, 20) : keccak_256(account).subarray(12);
  return getAddress(toHex(raw));
};

export interface DeployOptions {
  /** Contract compiled to PolkaVM (`.polkavm` from `resolc --bin`). */
  code: Uint8Array;
  abi: Abi;
  /** Arguments of the constructor. */
  args?: readonly unknown[];
  /** Sent to the contract with its constructor, which must then be `payable`. */
  value?: Amount;
  /** With a salt (32 bytes), the address of the contract depends only on the deployer, the
   * code, the constructor arguments and the salt; without, on the deployer and its nonce. */
  salt?: Uint8Array;
}

/** Result of a dry run. */
export interface DryRun {
  /** Data returned, or the revert data when `reverted`. */
  output: Uint8Array;
  reverted: boolean;
  /** Weight the call needs: time (picoseconds) and proof size (bytes). */
  weight: { refTime: bigint; proofSize: bigint };
  /** Most storage deposit the call charges at once. */
  storageDeposit: Amount;
  /** Address of the contract, for a deployment. */
  contract?: Hex;
}

/** An event a contract emitted, decoded with its ABI when it declares it. */
export interface ContractEvent {
  contract: Hex;
  /** Name of the event, when the ABI declares it. */
  name?: string;
  /** Arguments by name, indexed ones included. */
  args: Record<string, unknown>;
  topics: Hex[];
  data: Hex;
  blockNumber: number;
  blockHash: string;
}

/** Result of a transaction to a contract. */
export interface ContractTxOutcome extends TxOutcome {
  /** Events emitted by the contracts the transaction called. */
  contractEvents: ContractEvent[];
}

const withMargin = (value: bigint) => value + (value * MARGIN_PERCENT + 99n) / 100n;

const charge = (deposit: { type: string; value: bigint }) => (deposit.type === "Charge" ? deposit.value : 0n);

const bytes = (value: unknown): Uint8Array =>
  value instanceof Uint8Array ? value : typeof value === "string" ? fromHex(value) : (value as { asBytes(): Uint8Array }).asBytes();

const hexOf = (value: unknown): Hex => (typeof value === "string" ? (value.toLowerCase() as Hex) : (toHex(bytes(value)) as Hex));

/** Whether the return flags of an execution have the revert bit. */
const reverted = (flags: unknown) => {
  const bits = typeof flags === "object" && flags !== null && "bits" in flags ? (flags as { bits: number }).bits : (flags as number);
  return (Number(bits) & 1) === 1;
};

const PANICS: Record<number, string> = {
  0x01: "assertion failed",
  0x11: "arithmetic overflow or underflow",
  0x12: "division by zero",
  0x21: "invalid enum value",
  0x22: "invalid storage byte array",
  0x31: "pop on an empty array",
  0x32: "index out of bounds",
  0x41: "out of memory",
  0x51: "call to an uninitialized function",
};

const show = (value: unknown): string =>
  typeof value === "string" && !value.startsWith("0x")
    ? JSON.stringify(value)
    : Array.isArray(value)
      ? `[${value.map(show).join(", ")}]`
      : String(value);

/** The reason a call reverted, from its revert data: `Error(string)`, `Panic(uint256)`, or an
 * error the ABI declares. */
export const revertReason = (abi: Abi | undefined, data: Uint8Array): string => {
  if (data.length === 0) return "reverted without a reason";
  try {
    const decoded = decodeErrorResult({ abi: abi ?? [], data: toHex(data) as Hex });
    const args = (decoded.args ?? []) as unknown[];
    if (decoded.errorName === "Error") return String(args[0]);
    if (decoded.errorName === "Panic") {
      const code = Number(args[0]);
      return `panic 0x${code.toString(16)}${PANICS[code] ? ` (${PANICS[code]})` : ""}`;
    }
    const inputs = "inputs" in decoded.abiItem ? decoded.abiItem.inputs : [];
    return `${decoded.errorName}(${inputs.map((input, i) => `${input.name}: ${show(args[i])}`).join(", ")})`;
  } catch {
    return `reverted: ${toHex(data)}`;
  }
};

/** Contract calls of a client. */
export class Contracts {
  constructor(private readonly client: TesseraxClient) {}

  /** Dry run of a call of `contract` with `data` by `origin`, on the state of the best block. */
  async dryRunCall(origin: string, contract: string, data: Uint8Array, value = new Amount(0n)): Promise<DryRun> {
    const result = await this.client.api.apis.ReviveApi.call(
      formatAddress(parseAddress(origin)),
      contract.toLowerCase(),
      value.planck,
      undefined,
      undefined,
      data,
      { at: "best" },
    );
    if (!result.result.success) throw new TransactionFailed(`dry run failed: ${chainError(result.result.value)}`, chainError(result.result.value));
    const output = result.result.value;
    return {
      output: bytes(output.data),
      reverted: reverted(output.flags),
      weight: { refTime: result.weight_required.ref_time, proofSize: result.weight_required.proof_size },
      storageDeposit: new Amount(charge(result.max_storage_deposit)),
    };
  }

  /** Dry run of the deployment of `code` with the constructor input `data` by `origin`. */
  async dryRunDeploy(origin: string, code: Uint8Array, data: Uint8Array, value = new Amount(0n), salt?: Uint8Array): Promise<DryRun> {
    const result = await this.client.api.apis.ReviveApi.instantiate(
      formatAddress(parseAddress(origin)),
      value.planck,
      undefined,
      undefined,
      Enum("Upload", code),
      data,
      salt ? toHex(salt) : undefined,
      { at: "best" },
    );
    if (!result.result.success) throw new TransactionFailed(`dry run failed: ${chainError(result.result.value)}`, chainError(result.result.value));
    const output = result.result.value;
    return {
      output: bytes(output.result.data),
      reverted: reverted(output.result.flags),
      weight: { refTime: result.weight_required.ref_time, proofSize: result.weight_required.proof_size },
      storageDeposit: new Amount(charge(result.max_storage_deposit)),
      contract: getAddress(hexOf(output.addr)),
    };
  }

  /** Deploy a contract, signed by `signer`, and return it once a block includes the deployment. */
  async deploy(signer: Keypair, options: DeployOptions): Promise<{ contract: Contract; outcome: ContractTxOutcome }> {
    const constructor = options.abi.find((item): item is AbiConstructor => item.type === "constructor");
    const args = options.args ?? [];
    if (!constructor && args.length > 0) throw new RangeError("the contract has no constructor to take arguments");
    const data = constructor ? fromHex(encodeAbiParameters(constructor.inputs, args as unknown[])) : new Uint8Array();
    const value = options.value ?? new Amount(0n);
    const dry = await this.dryRunDeploy(signer.address, options.code, data, value, options.salt);
    if (dry.reverted) throw new ContractReverted(revertReason(options.abi, dry.output), dry.output);
    const tx = this.client.api.tx.Revive.instantiate_with_code({
      value: value.planck,
      weight_limit: { ref_time: withMargin(dry.weight.refTime), proof_size: withMargin(dry.weight.proofSize) },
      storage_deposit_limit: withMargin(dry.storageDeposit.planck),
      code: options.code,
      data,
      salt: options.salt ? toHex(options.salt) : undefined,
    });
    const outcome = await this.client.submit(tx, signer);
    const instantiated = outcome.events.find((e) => e.type === "Revive" && e.value.type === "Instantiated");
    const address = instantiated ? getAddress(hexOf((instantiated.value.value as { contract: unknown }).contract)) : dry.contract!;
    const contract = new Contract(this.client, address, options.abi);
    return { contract, outcome: { ...outcome, contractEvents: contract.emitted(outcome) } };
  }

  /** Call `contract` with the encoded input `data`, signed by `signer`, after a dry run that stops
   * a call that would revert. */
  async call(signer: Keypair, contract: string, data: Uint8Array, value = new Amount(0n), abi?: Abi): Promise<TxOutcome> {
    const dry = await this.dryRunCall(signer.address, contract, data, value);
    if (dry.reverted) throw new ContractReverted(revertReason(abi, dry.output), dry.output);
    const tx = this.client.api.tx.Revive.call({
      dest: contract.toLowerCase(),
      value: value.planck,
      weight_limit: { ref_time: withMargin(dry.weight.refTime), proof_size: withMargin(dry.weight.proofSize) },
      storage_deposit_limit: withMargin(dry.storageDeposit.planck),
      data,
    });
    return this.client.submit(tx, signer);
  }

  /** Hash of the block `number` of the best chain, if the chain has it. */
  async blockHash(number: number): Promise<string | undefined> {
    return (await this.client.client._request<string | null>("chain_getBlockHash", [number])) ?? undefined;
  }

  /** Events emitted by `contract`, if given, in the blocks `from` to `to` of the best chain, as
   * raw events. The node must keep the state of these blocks, as an archive node does. */
  async rawEvents(contract: string | undefined, from: number, to: number) {
    if (to < from || to - from >= MAX_EVENT_RANGE) throw new RangeError(`read at most ${MAX_EVENT_RANGE} blocks at once, from <= to`);
    const found: { blockNumber: number; blockHash: string; contract: Hex; topics: Hex[]; data: Hex }[] = [];
    for (let number = from; number <= to; number++) {
      const hash = await this.blockHash(number);
      if (!hash) break;
      found.push(...(await this.emittedAt(hash, number)).filter((e) => !contract || e.contract === getAddress(contract)));
    }
    return found;
  }

  /** Contract events of the block `hash`. */
  async emittedAt(hash: string, number: number) {
    const records = (await this.client.api.query.System.Events.getValue({ at: hash })) as { event: { type: string; value: { type: string; value: any } } }[];
    return records
      .filter((r) => r.event.type === "Revive" && r.event.value.type === "ContractEmitted")
      .map((r) => rawEvent(r.event.value.value, number, hash));
  }
}

const rawEvent = (value: { contract: unknown; data: unknown; topics: unknown[] }, blockNumber: number, blockHash: string) => ({
  blockNumber,
  blockHash,
  contract: getAddress(hexOf(value.contract)),
  topics: value.topics.map(hexOf),
  data: hexOf(value.data),
});

/** A deployed contract and its ABI. */
export class Contract {
  constructor(
    private readonly client: TesseraxClient,
    readonly address: Hex,
    readonly abi: Abi,
  ) {}

  /** Input of a call of `functionName` with `args`: its selector and its encoded arguments. */
  encode(functionName: string, args: readonly unknown[] = []): Uint8Array {
    return fromHex(encodeFunctionData({ abi: this.abi, functionName, args } as Parameters<typeof encodeFunctionData>[0]));
  }

  /** What `functionName` returned in `output`: its value, or an array for several outputs. */
  decodeOutput(functionName: string, output: Uint8Array): unknown {
    return decodeFunctionResult({ abi: this.abi, functionName, data: toHex(output) as Hex } as Parameters<typeof decodeFunctionResult>[0]);
  }

  /** Call `functionName` with `args` without a transaction, and return what it returns. The
   * caller (`msg.sender`) is `from`, or the zero account. */
  async read(functionName: string, args: readonly unknown[] = [], options: { from?: string } = {}): Promise<any> {
    const dry = await this.client.contracts.dryRunCall(options.from ?? ZERO_ACCOUNT, this.address, this.encode(functionName, args));
    if (dry.reverted) throw new ContractReverted(revertReason(this.abi, dry.output), dry.output);
    return this.decodeOutput(functionName, dry.output);
  }

  /** Call `functionName` with `args` in a transaction signed by `signer`, sending `value` to a
   * `payable` function, and return once a block includes it, with the events it emitted. */
  async write(signer: Keypair, functionName: string, args: readonly unknown[] = [], options: { value?: Amount } = {}): Promise<ContractTxOutcome> {
    const outcome = await this.client.contracts.call(signer, this.address, this.encode(functionName, args), options.value, this.abi);
    return { ...outcome, contractEvents: this.emitted(outcome) };
  }

  /** The event of `topics` and `data`, decoded with the ABI. */
  decodeEvent(topics: Hex[], data: Hex): { name: string; args: Record<string, unknown> } | undefined {
    try {
      const decoded = decodeEventLog({ abi: this.abi, topics: topics as [Hex, ...Hex[]], data, strict: true }) as unknown as {
        eventName: string;
        args?: Record<string, unknown> | readonly unknown[];
      };
      // Events with unnamed arguments have them by position.
      const args = Array.isArray(decoded.args)
        ? Object.fromEntries(decoded.args.map((value, i) => [String(i), value]))
        : { ...decoded.args };
      return { name: decoded.eventName, args };
    } catch {
      return undefined;
    }
  }

  private event(raw: ReturnType<typeof rawEvent>): ContractEvent {
    const decoded = this.decodeEvent(raw.topics, raw.data);
    return { ...raw, name: decoded?.name, args: decoded?.args ?? {} };
  }

  /** Contract events of a transaction outcome. */
  emitted(outcome: TxOutcome): ContractEvent[] {
    return outcome.events
      .filter((e) => e.type === "Revive" && e.value.type === "ContractEmitted")
      .map((e) => this.event(rawEvent(e.value.value as any, outcome.blockNumber, outcome.blockHash)));
  }

  /** Events this contract emitted in the blocks `from` to `to` of the best chain, at most
   * `MAX_EVENT_RANGE` blocks at once. */
  async events(from: number, to: number): Promise<ContractEvent[]> {
    return (await this.client.contracts.rawEvents(this.address, from, to)).map((raw) => this.event(raw));
  }

  /** Call `onEvent` with the events this contract emits from now on, block after block of the
   * best chain, and return a function that stops. A block of the best chain may still leave it,
   * with its events, until it is 60 blocks deep. */
  watchEvents(onEvent: (event: ContractEvent) => void, onError: (error: Error) => void = () => {}): () => void {
    let last: number | undefined;
    let queue = Promise.resolve();
    const subscription = this.client.client.bestBlocks$.subscribe({
      next: (blocks) => {
        const best = blocks[0];
        // The first notification is the current best block, whose events are past.
        if (last === undefined) {
          last = best.number;
          return;
        }
        if (best.number <= last) return;
        const [from, to] = [last + 1, best.number];
        last = best.number;
        queue = queue.then(async () => {
          try {
            for (let number = from; number <= to; number++) {
              const hash = number === to ? best.hash : await this.client.contracts.blockHash(number);
              if (!hash) continue;
              for (const raw of await this.client.contracts.emittedAt(hash, number)) {
                if (raw.contract === this.address) onEvent(this.event(raw));
              }
            }
          } catch (error) {
            onError(error as Error);
          }
        });
      },
      error: (error) => onError(error as Error),
    });
    return () => subscription.unsubscribe();
  }
}
