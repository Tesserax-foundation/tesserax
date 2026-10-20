use crate as pallet_agent_registry;
use frame_support::{
	derive_impl,
	traits::ConstU64,
};
use sp_runtime::BuildStorage;

type Block = frame_system::mocking::MockBlock<Test>;

pub const OWNER: u64 = 1;
pub const AGENT: u64 = 2;
pub const SHOP: u64 = 3;
pub const OTHER: u64 = 4;
pub const DEPOSIT_BASE: u64 = 10;
pub const DEPOSIT_PER_BYTE: u64 = 1;
pub const MIN_PAYMENT: u64 = 5;

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
	pub type Registry = pallet_agent_registry;
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
	type RuntimeHoldReason = RuntimeHoldReason;
}

impl pallet_agent_registry::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type DepositBase = ConstU64<DEPOSIT_BASE>;
	type DepositPerByte = ConstU64<DEPOSIT_PER_BYTE>;
	type MinPayment = ConstU64<MIN_PAYMENT>;
	type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(OWNER, 10_000), (AGENT, 1_000), (SHOP, 100), (OTHER, 100)],
		..Default::default()
	}
	.assimilate_storage(&mut storage)
	.unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| System::set_block_number(1));
	ext
}
