use crate as pallet_shielded;
use frame_support::{
	derive_impl,
	traits::{ConstU32, ConstU64, FindAuthor},
};
use sp_runtime::{BuildStorage, ConsensusEngineId};

type Block = frame_system::mocking::MockBlock<Test>;

pub const AUTHOR: u64 = 99;
pub const MINIMUM_FEE: u64 = 1_000;
pub const ROOT_HISTORY: u32 = 4;

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
	pub type Shielded = pallet_shielded;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type AccountStore = System;
	type ExistentialDeposit = ConstU64<10>;
}

pub struct TestAuthor;

impl FindAuthor<u64> for TestAuthor {
	fn find_author<'a, I>(_digests: I) -> Option<u64>
	where
		I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
	{
		Some(AUTHOR)
	}
}

impl pallet_shielded::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type FindAuthor = TestAuthor;
	type RootHistory = ConstU32<ROOT_HISTORY>;
	type MinimumFee = ConstU64<MINIMUM_FEE>;
	type Longevity = ConstU64<64>;
	type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(1, 1_000_000), (2, 1_000_000), (AUTHOR, 1_000)],
		..Default::default()
	}
	.assimilate_storage(&mut storage)
	.unwrap();
	pallet_shielded::GenesisConfig::<Test>::default().assimilate_storage(&mut storage).unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| System::set_block_number(1));
	ext
}
