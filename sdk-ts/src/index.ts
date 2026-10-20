/**
 * TypeScript SDK of Tesserax: ML-DSA accounts, transfers, AI agent delegations and the agent
 * registry.
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
