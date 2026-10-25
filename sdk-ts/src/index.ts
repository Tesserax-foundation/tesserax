/**
 * TypeScript SDK of Tesserax: ML-DSA accounts, transfers, AI agent delegations, the agent
 * registry, smart contracts and private transactions (`@tesserax/sdk/shielded`, with its
 * WebAssembly module).
 *
 * ```ts
 * import { Amount, Keypair, TesseraxClient } from "@tesserax/sdk";
 *
 * const client = new TesseraxClient("ws://127.0.0.1:9944");
 * const agent = Keypair.fromUri(process.env.TESSERAX_KEY!);
 * await client.agentTransfer(agent, "te...owner", "te...merchant", Amount.fromTess("1.5"));
 * ```
 */

export { Amount, DECIMALS, PLANCK_PER_TESS } from "./amount.js";
export {
  Keypair,
  SIGNING_CONTEXT,
  SS58_PREFIX,
  accountIdOf,
  formatAddress,
  parseAddress,
  verify,
} from "./keys.js";
export { mlDsaTxCreator } from "./signer.js";
export {
  CLOSE_SAFETY_BLOCKS,
  ChannelPayer,
  ChannelReceiver,
  Channels,
  PAYMENT_HEADER,
  VOUCHER_CONTEXT,
  VOUCHER_HEADER,
  Voucher,
  paymentRequiredHeader,
  parsePaymentRequired,
  voucherMessage,
  type ChannelInfo,
  type PayerChannel,
  type PaymentRequired,
  type Rejection,
} from "./channels.js";
export {
  Contract,
  ContractReverted,
  Contracts,
  MAX_EVENT_RANGE,
  revertReason,
  toH160,
  type ContractEvent,
  type ContractTxOutcome,
  type DeployOptions,
  type DryRun,
} from "./contracts.js";
export { StorageError } from "./store.js";
export { Tasks, blocksFor, type NewTask, type TaskInfo, type TaskStatus } from "./tasks.js";
export {
  MORTALITY,
  TesseraxClient,
  TransactionFailed,
  type AgentRecord,
  type Balance,
  type ChainInfo,
  type Delegation,
  type Policy,
  type Profile,
  type Reputation,
  type TxOutcome,
} from "./client.js";
