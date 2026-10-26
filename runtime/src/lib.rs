//! The Tesserax runtime.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

pub mod apis;
#[cfg(feature = "runtime-benchmarks")]
mod benchmarks;
pub mod configs;
pub mod genesis_config_presets;
pub mod governance;
mod migrations;
pub mod precompiles;
pub mod weights;
#[cfg(test)]
mod tests;

extern crate alloc;
use alloc::vec::Vec;

use sp_runtime::{
	generic,
	traits::{BlakeTwo256, IdentifyAccount, Verify},
	MultiAddress,
};
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

pub use frame_system::Call as SystemCall;
pub use pallet_balances::Call as BalancesCall;
pub use pallet_timestamp::Call as TimestampCall;
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;

/// Opaque types, used by the node without knowledge of the runtime internals.
pub mod opaque {
	use super::*;
	use sp_runtime::traits::Hash as HashT;

	pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

	/// Opaque block header type.
	pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
	/// Opaque block type.
	pub type Block = generic::Block<Header, UncheckedExtrinsic>;
	/// Opaque block identifier type.
	pub type BlockId = generic::BlockId<Block>;
	/// Opaque block hash type.
	pub type Hash = <BlakeTwo256 as HashT>::Output;
}

#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
	spec_name: alloc::borrow::Cow::Borrowed("tesserax"),
	impl_name: alloc::borrow::Cow::Borrowed("tesserax-node"),
	authoring_version: 1,
	spec_version: 105,
	impl_version: 1,
	apis: apis::RUNTIME_API_VERSIONS,
	transaction_version: 1,
	system_version: 1,
};

/// Target block time, in milliseconds. Proof of work makes actual block times vary around it.
pub const MILLI_SECS_PER_BLOCK: u64 = 18_000;

// Time is measured by number of blocks. An hour is counted from milliseconds: a minute is 3.33
// blocks, and counting hours in whole minutes would make every period 10% short.
pub const HOURS: BlockNumber = (3_600_000 / MILLI_SECS_PER_BLOCK) as BlockNumber;
pub const DAYS: BlockNumber = HOURS * 24;

/// Number of decimals of the TESS token.
pub const DECIMALS: u8 = 12;

/// One TESS.
pub const UNIT: Balance = 1_000_000_000_000;
pub const MILLI_UNIT: Balance = 1_000_000_000;
pub const MICRO_UNIT: Balance = 1_000_000;

/// Existential deposit.
pub const EXISTENTIAL_DEPOSIT: Balance = MILLI_UNIT;

/// Total supply: 14 000 000 TESS.
pub const TOTAL_SUPPLY: Balance = 14_000_000 * UNIT;

/// Mining rewards: 85% of the supply, 11 900 000 TESS.
pub const MINING_SUPPLY: Balance = 11_900_000 * UNIT;

/// Reward of each block before the first halving: 0.85 TESS.
pub const BLOCK_REWARD: Balance = 850 * MILLI_UNIT;

/// Number of blocks between two halvings of the block reward, about four years.
pub const HALVING_INTERVAL: BlockNumber = 7_000_000;

/// Blocks in a month: 30.4375 days of 18 s blocks.
pub const MONTH: BlockNumber = 146_100;

/// Team allocation: 10% of the supply, 1 400 000 TESS.
pub const TEAM_ALLOCATION: Balance = 1_400_000 * UNIT;

/// Nothing of the team allocation is released before six months.
pub const TEAM_CLIFF: BlockNumber = 6 * MONTH;

/// The team allocation is fully released after 48 months, cliff included.
pub const TEAM_VESTING: BlockNumber = 48 * MONTH;

/// Foundation allocation: 5% of the supply, 700 000 TESS.
pub const FOUNDATION_ALLOCATION: Balance = 700_000 * UNIT;

/// Part of the foundation allocation spendable from genesis.
pub const FOUNDATION_LIQUID: Balance = 140_000 * UNIT;

/// The rest of the foundation allocation is released linearly over 36 months, without cliff.
pub const FOUNDATION_VESTING: BlockNumber = 36 * MONTH;

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
	NativeVersion { runtime_version: VERSION, can_author_with: Default::default() }
}

/// Signature of a transaction: an ML-DSA-87 signature and the signer's public key. ML-DSA is the
/// only signature scheme accepted on Tesserax.
pub type Signature = tesserax_crypto::AccountSignature;

/// An account on the chain: the BLAKE2-256 hash of its ML-DSA public key.
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

/// SS58 address format of Tesserax accounts. Addresses start with `te`.
pub const SS58_PREFIX: u16 = 2261;

/// Balance of an account.
pub type Balance = u128;

/// Index of a transaction in the chain.
pub type Nonce = u32;

/// A hash of some data used by the chain.
pub type Hash = sp_core::H256;

/// An index to a block.
pub type BlockNumber = u32;

/// The address format for describing accounts.
pub type Address = MultiAddress<AccountId, ()>;

/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;

/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

/// A Block signed with a Justification
pub type SignedBlock = generic::SignedBlock<Block>;

/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;

/// The `TransactionExtension` to the basic transaction logic.
pub type TxExtension = (
	frame_system::AuthorizeCall<Runtime>,
	frame_system::CheckNonZeroSender<Runtime>,
	frame_system::CheckSpecVersion<Runtime>,
	frame_system::CheckTxVersion<Runtime>,
	frame_system::CheckGenesis<Runtime>,
	frame_system::CheckEra<Runtime>,
	frame_system::CheckNonce<Runtime>,
	frame_system::CheckWeight<Runtime>,
	pallet_transaction_payment::ChargeTransactionPayment<Runtime>,
	frame_metadata_hash_extension::CheckMetadataHash<Runtime>,
	frame_system::WeightReclaim<Runtime>,
);

/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
	generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;

/// The payload being signed in transactions.
pub type SignedPayload = generic::SignedPayload<RuntimeCall, TxExtension>;

/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
	Runtime,
	Block,
	frame_system::ChainContext<Runtime>,
	Runtime,
	AllPalletsWithSystem,
	migrations::Migrations,
>;

#[frame_support::runtime]
mod runtime {
	#[runtime::runtime]
	#[runtime::derive(
		RuntimeCall,
		RuntimeEvent,
		RuntimeError,
		RuntimeOrigin,
		RuntimeFreezeReason,
		RuntimeHoldReason,
		RuntimeSlashReason,
		RuntimeLockId,
		RuntimeTask,
		RuntimeViewFunction
	)]
	pub struct Runtime;

	#[runtime::pallet_index(0)]
	pub type System = frame_system;

	#[runtime::pallet_index(1)]
	pub type Timestamp = pallet_timestamp;

	#[runtime::pallet_index(2)]
	pub type Difficulty = pallet_difficulty;

	#[runtime::pallet_index(3)]
	pub type Balances = pallet_balances;

	#[runtime::pallet_index(4)]
	pub type TransactionPayment = pallet_transaction_payment;

	#[runtime::pallet_index(6)]
	pub type Revive = pallet_revive;

	#[runtime::pallet_index(7)]
	pub type Shielded = pallet_shielded;

	#[runtime::pallet_index(8)]
	pub type Emission = pallet_emission;

	#[runtime::pallet_index(9)]
	pub type Vesting = pallet_vesting;

	#[runtime::pallet_index(10)]
	pub type Multisig = pallet_multisig;

	#[runtime::pallet_index(11)]
	pub type Allocations = pallet_allocations;

	#[runtime::pallet_index(12)]
	pub type Agents = pallet_agents;

	#[runtime::pallet_index(13)]
	pub type AgentRegistry = pallet_agent_registry;

	#[runtime::pallet_index(14)]
	pub type Channels = pallet_channels;

	#[runtime::pallet_index(15)]
	pub type Tasks = pallet_tasks;

	// On-chain governance; index 5 was sudo.
	#[runtime::pallet_index(16)]
	pub type Preimage = pallet_preimage;

	#[runtime::pallet_index(17)]
	pub type Scheduler = pallet_scheduler;

	#[runtime::pallet_index(18)]
	pub type Referenda = pallet_referenda;

	#[runtime::pallet_index(19)]
	pub type ConvictionVoting = pallet_conviction_voting;

	#[runtime::pallet_index(20)]
	pub type Origins = governance::origins;

	#[runtime::pallet_index(21)]
	pub type Whitelist = pallet_whitelist;

	#[runtime::pallet_index(22)]
	pub type TechnicalCommittee = pallet_collective<Instance1>;

	#[runtime::pallet_index(23)]
	pub type Swaps = pallet_swaps;
}
