use crate as pallet_tasks;
use frame_support::{
	derive_impl, parameter_types,
	traits::{ConstU128, ConstU64},
	PalletId,
};
use sp_runtime::BuildStorage;
use std::cell::RefCell;

type Block = frame_system::mocking::MockBlock<Test>;

pub const CLIENT: u64 = 1;
pub const WORKER: u64 = 2;
pub const ARBITER: u64 = 3;
pub const OTHER: u64 = 4;
pub const MIN_REWARD: u128 = 100;
pub const STORAGE_DEPOSIT: u128 = 5;
pub const ARBITRATION_PERIOD: u64 = 20;

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
	pub type Tasks = pallet_tasks;
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

thread_local! {
	/// Outcomes recorded in the reputation of workers.
	pub static RECORDED: RefCell<Vec<(u64, bool)>> = const { RefCell::new(Vec::new()) };
}

pub struct Recorder;

impl pallet_tasks::ReputationRecorder<u64> for Recorder {
	fn record_task(worker: &u64, completed: bool) {
		RECORDED.with(|r| r.borrow_mut().push((*worker, completed)));
	}
}

parameter_types! {
	pub const TasksPalletId: PalletId = PalletId(*b"tess/tsk");
}

impl pallet_tasks::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type Reputation = Recorder;
	type PalletId = TasksPalletId;
	type MinReward = ConstU128<MIN_REWARD>;
	type StorageDeposit = ConstU128<STORAGE_DEPOSIT>;
	type MinPeriod = ConstU64<5>;
	type MaxPeriod = ConstU64<1_000>;
	type ArbitrationPeriod = ConstU64<ARBITRATION_PERIOD>;
	type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(CLIENT, 10_000), (WORKER, 1_000), (ARBITER, 10), (OTHER, 1_000)],
		..Default::default()
	}
	.assimilate_storage(&mut storage)
	.unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| {
		System::set_block_number(1);
		RECORDED.with(|r| r.borrow_mut().clear());
	});
	ext
}
