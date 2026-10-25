//! Rust SDK of Tesserax.
//!
//! Connects to a Tesserax node, signs transactions with ML-DSA-87 keys and covers what an AI
//! agent needs: balances and transfers, delegations from an owner to an agent key, the agent
//! registry with its reputation, smart contracts ([`contracts`]) and governance
//! ([`governance`]).
//!
//! ```no_run
//! # async fn example() -> Result<(), tesserax_sdk::Error> {
//! use tesserax_sdk::{Client, Keypair, Amount};
//!
//! let client = Client::connect("ws://127.0.0.1:9944").await?;
//! let agent = Keypair::from_uri("//Charlie")?;
//! let owner = tesserax_sdk::parse_address("te...")?;
//! // Pay a merchant from the owner's funds, within the agent's delegation.
//! client.agent_transfer(&agent, &owner, &owner, Amount::from_tess("1.5")?).await?;
//! # Ok(()) }
//! ```

pub mod amount;
pub mod channels;
pub mod client;
pub mod contracts;
pub mod governance;
pub mod keys;
#[cfg(feature = "shielded")]
pub mod shielded;
mod store;
pub mod swaps;
pub mod tasks;

pub use amount::Amount;
pub use channels::{
	ChannelInfo, ChannelPayer, ChannelReceiver, PaymentRequired, PayerChannel, Rejection, Voucher,
	PAYMENT_HEADER, VOUCHER_HEADER,
};
pub use client::{
	AgentRecord, Balance, ChainInfo, Client, Delegation, Permissions, Policy, Profile, Reputation,
	TxOutcome,
};
pub use contracts::{Contract, ContractEvent, ContractTx, Deploy, DryRun};
pub use keys::{format_address, parse_address, AccountId, Keypair, SS58_PREFIX};
pub use tasks::{NewTask, TaskInfo, TaskStatus, BLOCKS_PER_HOUR};

/// Types and calls generated from the runtime metadata.
#[subxt::subxt(
	runtime_metadata_path = "metadata/tesserax.scale",
	derive_for_all_types = "Clone, Debug"
)]
pub mod runtime {}

/// Configuration of the Tesserax chain for `subxt`: ML-DSA-87 account signatures.
pub enum TesseraxConfig {}

impl subxt::Config for TesseraxConfig {
	type AccountId = subxt::utils::AccountId32;
	type Address = subxt::utils::MultiAddress<Self::AccountId, ()>;
	type Signature = tesserax_crypto::AccountSignature;
	type Hasher = subxt::config::substrate::BlakeTwo256;
	type Header = subxt::config::substrate::SubstrateHeader<u32, Self::Hasher>;
	type ExtrinsicParams = subxt::config::DefaultExtrinsicParams<Self>;
	type AssetId = u32;
}

/// Errors of the SDK.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("node error: {0}")]
	Node(#[from] subxt::Error),
	#[error("invalid address: {0}")]
	InvalidAddress(String),
	#[error("invalid amount: {0}")]
	InvalidAmount(String),
	#[error("invalid key: {0}")]
	InvalidKey(String),
	#[error("invalid input: {0}")]
	InvalidInput(String),
	#[error("transaction failed: {0}")]
	Dispatch(String),
	/// A contract reverted, with its reason, such as `InsufficientBalance(available: 5, required: 10)`.
	#[error("contract reverted: {0}")]
	ContractReverted(String),
	#[error("not found: {0}")]
	NotFound(String),
	/// An EVM chain of a swap answered with an error.
	#[error("EVM chain: {0}")]
	Evm(String),
	/// Bitcoin, or the Esplora API of a swap, answered with an error.
	#[error("Bitcoin: {0}")]
	Bitcoin(String),
	#[error("state file: {0}")]
	Storage(String),
}

pub type Result<T> = core::result::Result<T, Error>;
