use crate as pallet_difficulty;
use frame_support::{
	derive_impl,
	traits::{ConstU32, ConstU64, Hooks},
};
use sp_core::U256;
use sp_runtime::BuildStorage;

type Block = frame_system::mocking::MockBlock<Test>;

pub const TARGET: u64 = 18_000;
pub const WINDOW: u32 = 120;
pub const MINIMUM: u64 = 1_000;
pub const INITIAL: u64 = 1_000_000;

#[frame_support::runtime]
mod runtime {
	#[runtime::runtime]
	#[runtime::derive(RuntimeCall, RuntimeEvent, RuntimeError, RuntimeOrigin, RuntimeTask)]
	pub struct Test;

	#[runtime::pallet_index(0)]
	pub type System = frame_system;

	#[runtime::pallet_index(1)]
	pub type Timestamp = pallet_timestamp;

	#[runtime::pallet_index(2)]
	pub type Difficulty = pallet_difficulty;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
}

impl pallet_timestamp::Config for Test {
	type Moment = u64;
	type OnTimestampSet = ();
	type MinimumPeriod = ConstU64<1>;
	type WeightInfo = ();
}

frame_support::parameter_types! {
	pub const MinimumDifficulty: U256 = U256([MINIMUM, 0, 0, 0]);
}

impl pallet_difficulty::Config for Test {
	type Time = Timestamp;
	type TargetBlockTime = ConstU64<TARGET>;
	type Window = ConstU32<WINDOW>;
	type MinimumDifficulty = MinimumDifficulty;
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let storage = RuntimeGenesisConfig {
		difficulty: pallet_difficulty::GenesisConfig {
			initial_difficulty: U256::from(INITIAL),
			..Default::default()
		},
		..Default::default()
	}
	.build_storage()
	.unwrap();
	storage.into()
}

/// Produce a block whose timestamp is `solve_time` after the previous one.
pub fn produce_block(solve_time: u64) {
	let number = System::block_number() + 1;
	System::set_block_number(number);
	let now = pallet_timestamp::Now::<Test>::get() + solve_time;
	pallet_timestamp::Now::<Test>::put(now);
	Difficulty::on_finalize(number);
}
