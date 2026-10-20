//! Rust SDK of Tesserax.
//!
//! Connects to a Tesserax node, signs transactions with ML-DSA-65 keys and covers what an AI
//! agent needs: balances and transfers, delegations from an owner to an agent key, and the agent
//! registry with its reputation.
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
pub mod client;
pub mod keys;

pub use amount::Amount;
pub use client::{
	AgentRecord, Balance, ChainInfo, Client, Delegation, Permissions, Policy, Profile, Reputation,
	TxOutcome,
};
pub use keys::{format_address, parse_address, AccountId, Keypair, SS58_PREFIX};

/// Types and calls generated from the runtime metadata.
#[subxt::subxt(
	runtime_metadata_path = "metadata/tesserax.scale",
	derive_for_all_types = "Clone, Debug"
)]
pub mod runtime {}

/// Configuration of the Tesserax chain for `subxt`: ML-DSA-65 account signatures.
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
	#[error("not found: {0}")]
	NotFound(String),
}

pub type Result<T> = core::result::Result<T, Error>;
