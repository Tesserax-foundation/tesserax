use crate as pallet_channels;
use frame_support::{
	derive_impl,
	traits::{ConstU128, ConstU64},
};
use sp_runtime::BuildStorage;

type Block = frame_system::mocking::MockBlock<Test>;

pub const PAYER: u64 = 1;
pub const PAYEE: u64 = 2;
pub const OTHER: u64 = 3;
pub const CLOSE_PERIOD: u64 = 10;
pub const MIN_DEPOSIT: u128 = 100;
pub const STORAGE_DEPOSIT: u128 = 5;

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
	pub type Channels = pallet_channels;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountData = pallet_balances::AccountData<u128>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type Balance = u128;
	type AccountStore = System;
	type ExistentialDeposit = ConstU128<1>;
	type RuntimeHoldReason = RuntimeHoldReason;
}

impl pallet_channels::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type ClosePeriod = ConstU64<CLOSE_PERIOD>;
	type MinDeposit = ConstU128<MIN_DEPOSIT>;
	type StorageDeposit = ConstU128<STORAGE_DEPOSIT>;
	type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(PAYER, 10_000), (PAYEE, 10), (OTHER, 10)],
		..Default::default()
	}
	.assimilate_storage(&mut storage)
	.unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| System::set_block_number(1));
	ext
}
