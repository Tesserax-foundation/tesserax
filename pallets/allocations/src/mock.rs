use crate as pallet_allocations;
use frame_support::{
	derive_impl, parameter_types,
	traits::{ConstU64, WithdrawReasons},
};
use sp_runtime::{traits::ConvertInto, BuildStorage};

type Block = frame_system::mocking::MockBlock<Test>;

pub const TEAM: u64 = 1;
pub const FOUNDATION: u64 = 2;
pub const OTHER: u64 = 3;

/// Team: 4 800 over 48 blocks with a cliff at block 6.
pub const TEAM_AMOUNT: u64 = 4_800;
pub const TEAM_CLIFF: u64 = 6;
pub const TEAM_DURATION: u64 = 48;

/// Foundation: 700, of which 140 liquid, the rest over 36 blocks without cliff.
pub const FOUNDATION_AMOUNT: u64 = 700;
pub const FOUNDATION_LIQUID: u64 = 140;
pub const FOUNDATION_DURATION: u64 = 36;

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
	pub struct Test;

	#[runtime::pallet_index(0)]
	pub type System = frame_system;

	#[runtime::pallet_index(1)]
	pub type Balances = pallet_balances;

	#[runtime::pallet_index(2)]
	pub type Vesting = pallet_vesting;

	#[runtime::pallet_index(3)]
	pub type Allocations = pallet_allocations;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type AccountStore = System;
	type ExistentialDeposit = ConstU64<1>;
}

parameter_types! {
	pub UnvestedFundsAllowedWithdrawReasons: WithdrawReasons =
		WithdrawReasons::except(WithdrawReasons::TRANSFER | WithdrawReasons::RESERVE);
}

impl pallet_vesting::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type BlockNumberToBalance = ConvertInto;
	type MinVestedTransfer = ConstU64<10>;
	type WeightInfo = ();
	type UnvestedFundsAllowedWithdrawReasons = UnvestedFundsAllowedWithdrawReasons;
	type BlockNumberProvider = System;
	const MAX_VESTING_SCHEDULES: u32 = 28;
}

impl pallet_allocations::Config for Test {
	type Currency = Balances;
	type Vesting = Vesting;
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> { balances: vec![(OTHER, 100)], ..Default::default() }
		.assimilate_storage(&mut storage)
		.unwrap();
	pallet_allocations::GenesisConfig::<Test> {
		allocations: vec![
			(TEAM, TEAM_AMOUNT, 0, TEAM_CLIFF, TEAM_DURATION),
			(FOUNDATION, FOUNDATION_AMOUNT, FOUNDATION_LIQUID, 0, FOUNDATION_DURATION),
		],
	}
	.assimilate_storage(&mut storage)
	.unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| System::set_block_number(1));
	ext
}
