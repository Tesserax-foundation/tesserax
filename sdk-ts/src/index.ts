/**
 * TypeScript SDK of Tesserax: ML-DSA accounts, transfers, AI agent delegations, the agent
 * registry, smart contracts, atomic swaps and private transactions (`@tesserax/sdk/shielded`,
 * with its WebAssembly module).
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
export {
  BITCOIN_LOCK_SECONDS,
  Esplora,
  MIN_CONFIRMATIONS,
  NOTICE_SATS,
  addressOfKey,
  bitcoinNetworkFor,
  claimTransaction,
  decodeAnnouncement,
  defaultEsplora,
  encodeAnnouncement,
  findLock,
  makerKey,
  extractSigned,
  htlcOutputScript,
  htlcScript,
  lockPsbt,
  outpointBytes,
  refundKey,
  refundTransaction,
  type Announcement,
  type BitcoinLock,
  type BitcoinNetwork,
  type Htlc,
  type PayingUtxo,
  type ReadAnnouncement,
} from "./bitcoin.js";
export {
  EVM_CHAINS,
  EvmSwaps,
  LOG_RANGE,
  SWAP_ABI,
  SWAP_CONTRACT,
  Swaps,
  assetOf,
  evmNetworks,
  hashOf,
  newSecret,
  paymentDecimals,
  paymentDue,
  paymentSymbol,
  tessFor,
  tokenOf,
  type Bid,
  type EvmNetwork,
  type LockState,
  type LockStatus,
  type Offer,
  type Payment,
  type PaymentLock,
  type Sale,
  type SwapAsset,
  type SwapChain,
  type SwapEvent,
  type Trade,
} from "./swaps.js";
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
