use codec::Decode;
use frame_support::{
	derive_impl,
	dispatch::DispatchClass,
	parameter_types,
	traits::{ConstBool, ConstU128, ConstU32, ConstU64, ConstU8, Contains, FindAuthor, VariantCountOf},
	weights::{
		constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
		IdentityFee, Weight,
	},
};
use frame_system::limits::{BlockLength, BlockWeights};
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use sp_core::U256;
use frame_system::EnsureSigned;
use pallet_revive::AccountId32Mapper;
use sp_runtime::{traits::One, ConsensusEngineId, FixedU128, Perbill};
use sp_version::RuntimeVersion;

use super::weights::{block_weights::BlockExecutionWeight, extrinsic_weights::ExtrinsicBaseWeight};
use super::{
	precompiles::MlDsa65Verify, AccountId, Balance, Balances, Block, BlockNumber, Hash, Nonce,
	PalletInfo, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason,
	RuntimeOrigin, RuntimeTask, System, Timestamp, EXISTENTIAL_DEPOSIT, MILLI_SECS_PER_BLOCK,
	MILLI_UNIT, SS58_PREFIX, VERSION,
};

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

/// Share of the block weight expected to be used by `on_initialize` hooks.
const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(10);

/// Two seconds of compute per block. Blocks must stay fast to import: every second spent
/// executing a block delays its propagation and raises the orphan rate.
const MAXIMUM_BLOCK_WEIGHT: Weight =
	Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX);

parameter_types! {
	pub const BlockHashCount: BlockNumber = 2400;
	pub const Version: RuntimeVersion = VERSION;

	/// Base weights benchmarked with ML-DSA transaction signatures.
	pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
		.base_block(BlockExecutionWeight::get())
		.for_class(DispatchClass::all(), |weights| {
			weights.base_extrinsic = ExtrinsicBaseWeight::get();
		})
		.for_class(DispatchClass::Normal, |weights| {
			weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
		})
		.for_class(DispatchClass::Operational, |weights| {
			weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
			// Operational transactions have some extra reserved space, so that they
			// are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
			weights.reserved = Some(
				MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT,
			);
		})
		.avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
		.build_or_panic();
	pub RuntimeBlockLength: BlockLength = BlockLength::builder()
		.max_length(5 * 1024 * 1024)
		.modify_max_length_for_class(DispatchClass::Normal, |m| *m = NORMAL_DISPATCH_RATIO * *m)
		.build();
	pub const SS58Prefix: u16 = SS58_PREFIX;
}

#[derive_impl(frame_system::config_preludes::SolochainDefaultConfig)]
impl frame_system::Config for Runtime {
	type Block = Block;
	type BlockWeights = RuntimeBlockWeights;
	type BlockLength = RuntimeBlockLength;
	type AccountId = AccountId;
	type Nonce = Nonce;
	type Hash = Hash;
	type BlockHashCount = BlockHashCount;
	type DbWeight = RocksDbWeight;
	type Version = Version;
	type AccountData = pallet_balances::AccountData<Balance>;
	type SS58Prefix = SS58Prefix;
	type MaxConsumers = ConstU32<16>;
	type BaseCallFilter = NoEthereumTransactions;
	/// Accounts get their contract address mapping on creation.
	type OnNewAccount = pallet_revive::AutoMapper<Runtime>;
	type OnKilledAccount = pallet_revive::AutoMapper<Runtime>;
}

/// Rejects the Ethereum transaction calls of `pallet_revive`. Ethereum transactions are signed
/// with ECDSA: contracts are deployed and called with ML-DSA signed extrinsics only.
pub struct NoEthereumTransactions;

impl Contains<RuntimeCall> for NoEthereumTransactions {
	fn contains(call: &RuntimeCall) -> bool {
		!matches!(
			call,
			RuntimeCall::Revive(
				pallet_revive::Call::eth_transact { .. } |
					pallet_revive::Call::eth_call { .. } |
					pallet_revive::Call::eth_instantiate_with_code { .. } |
					pallet_revive::Call::eth_substrate_call { .. }
			)
		)
	}
}

/// The miner of a block, read from the proof-of-work pre-runtime digest.
pub struct PowAuthor;

impl FindAuthor<AccountId> for PowAuthor {
	fn find_author<'a, I>(digests: I) -> Option<AccountId>
	where
		I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
	{
		digests.into_iter().find_map(|(id, mut data)| {
			(id == sp_consensus_pow::POW_ENGINE_ID).then(|| AccountId::decode(&mut data).ok())?
		})
	}
}

impl pallet_timestamp::Config for Runtime {
	/// Milliseconds since the unix epoch.
	type Moment = u64;
	type OnTimestampSet = ();
	/// Consecutive blocks are at least one second apart.
	type MinimumPeriod = ConstU64<1_000>;
	type WeightInfo = ();
}

parameter_types! {
	pub const MinimumDifficulty: U256 = U256([1_000, 0, 0, 0]);
}

impl pallet_difficulty::Config for Runtime {
	type Time = Timestamp;
	type TargetBlockTime = ConstU64<MILLI_SECS_PER_BLOCK>;
	/// 120 blocks, about 36 minutes.
	type Window = ConstU32<120>;
	type MinimumDifficulty = MinimumDifficulty;
}

impl pallet_balances::Config for Runtime {
	type MaxLocks = ConstU32<50>;
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	type Balance = Balance;
	type RuntimeEvent = RuntimeEvent;
	type DustRemoval = ();
	type ExistentialDeposit = ConstU128<EXISTENTIAL_DEPOSIT>;
	type AccountStore = System;
	type WeightInfo = pallet_balances::weights::SubstrateWeight<Runtime>;
	type FreezeIdentifier = RuntimeFreezeReason;
	type MaxFreezes = VariantCountOf<RuntimeFreezeReason>;
	type RuntimeHoldReason = RuntimeHoldReason;
	type RuntimeFreezeReason = RuntimeFreezeReason;
	type DoneSlashHandler = ();
}

parameter_types! {
	pub FeeMultiplier: Multiplier = Multiplier::one();
}

impl pallet_transaction_payment::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type OnChargeTransaction = FungibleAdapter<Balances, ()>;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightToFee = IdentityFee<Balance>;
	type LengthToFee = IdentityFee<Balance>;
	type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
	type WeightInfo = pallet_transaction_payment::weights::SubstrateWeight<Runtime>;
}

impl pallet_sudo::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type WeightInfo = pallet_sudo::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	/// Fees of private transactions are paid from the shielded pool, without signer: they must
	/// pay at least what a signed transaction of the same weight and length pays.
	pub ShieldedMinimumFee: u64 = shielded_minimum_fee();
}

/// Encoded length of a typical private transaction: proof, two encrypted notes, public inputs.
const SHIELDED_TRANSACTION_LENGTH: u64 = 190_000;

fn shielded_minimum_fee() -> u64 {
	use frame_support::weights::WeightToFee;
	use pallet_shielded::WeightInfo;
	type Weights = pallet_shielded::weights::SubstrateWeight<Runtime>;
	let weight = ExtrinsicBaseWeight::get()
		.saturating_add(Weights::authorize_transact())
		.saturating_add(Weights::transact());
	let fee = IdentityFee::<Balance>::weight_to_fee(&weight)
		.saturating_add(IdentityFee::<Balance>::weight_to_fee(&Weight::from_parts(
			SHIELDED_TRANSACTION_LENGTH,
			0,
		)));
	fee.try_into().unwrap_or(u64::MAX)
}

/// Private transfers: notes proven with zero-knowledge STARKs. See `docs/privacy.md`.
impl pallet_shielded::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type FindAuthor = PowAuthor;
	/// Anchors stay valid for the last 100 blocks that changed the note tree.
	type RootHistory = ConstU32<100>;
	type MinimumFee = ShieldedMinimumFee;
	type Longevity = ConstU64<64>;
	type WeightInfo = pallet_shielded::weights::SubstrateWeight<Runtime>;
}

/// Storage deposit for `items` storage items using `bytes` bytes.
const fn deposit(items: u32, bytes: u32) -> Balance {
	items as Balance * 20 * MILLI_UNIT + (bytes as Balance) * (MILLI_UNIT / 10)
}

parameter_types! {
	pub const DepositPerItem: Balance = deposit(1, 0);
	pub const DepositPerChildTrieItem: Balance = deposit(1, 0) / 100;
	pub const DepositPerByte: Balance = deposit(0, 1);
	pub const CodeHashLockupDepositPercent: Perbill = Perbill::from_percent(30);
	pub const MaxEthExtrinsicWeight: FixedU128 = FixedU128::from_rational(9, 10);
	pub const GasScale: u32 = 10;
}

/// Smart contracts: PolkaVM programs only, compiled from Solidity with `resolc`.
impl pallet_revive::Config for Runtime {
	type Time = Timestamp;
	type Balance = Balance;
	type Currency = Balances;
	type OnBurn = ();
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeHoldReason = RuntimeHoldReason;
	type WeightInfo = pallet_revive::weights::SubstrateWeight<Self>;
	type Precompiles = (MlDsa65Verify<Self>,);
	type FindAuthor = PowAuthor;
	type DepositPerByte = DepositPerByte;
	type DepositPerItem = DepositPerItem;
	type DepositPerChildTrieItem = DepositPerChildTrieItem;
	type CodeHashLockupDepositPercent = CodeHashLockupDepositPercent;
	type AddressMapper = AccountId32Mapper<Self>;
	/// No EVM bytecode: contracts run on PolkaVM.
	type AllowEVMBytecode = ConstBool<false>;
	type UploadOrigin = EnsureSigned<AccountId>;
	type InstantiateOrigin = EnsureSigned<AccountId>;
	type RuntimeMemory = ConstU32<{ 128 * 1024 * 1024 }>;
	type PVFMemory = ConstU32<{ 512 * 1024 * 1024 }>;
	/// Chain id returned to contracts by `block.chainid`.
	type ChainId = ConstU64<2261>;
	/// TESS has 12 decimals, contracts see 18.
	type NativeToEthRatio = ConstU32<1_000_000>;
	type FeeInfo = ();
	type Deposit = ();
	type MaxEthExtrinsicWeight = MaxEthExtrinsicWeight;
	type DebugEnabled = ConstBool<false>;
	type AutoMap = ConstBool<true>;
	type GasScale = GasScale;
}
