use frame_support::{
	derive_impl,
	dispatch::DispatchClass,
	parameter_types,
	traits::{ConstU128, ConstU32, ConstU64, ConstU8, VariantCountOf},
	weights::{
		constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
		IdentityFee, Weight,
	},
};
use frame_system::limits::{BlockLength, BlockWeights};
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use sp_core::U256;
use sp_runtime::{traits::One, Perbill};
use sp_version::RuntimeVersion;

use super::weights::{block_weights::BlockExecutionWeight, extrinsic_weights::ExtrinsicBaseWeight};
use super::{
	AccountId, Balance, Balances, Block, BlockNumber, Hash, Nonce, PalletInfo, Runtime,
	RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason, RuntimeOrigin, RuntimeTask,
	System, Timestamp, EXISTENTIAL_DEPOSIT, MILLI_SECS_PER_BLOCK, SS58_PREFIX, VERSION,
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
