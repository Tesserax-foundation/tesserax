use crate::{self as pallet_agents, CallKind, ClassifyCall};
use frame_support::{
	derive_impl,
	traits::{ConstU32, ConstU64},
};
use sp_runtime::BuildStorage;

type Block = frame_system::mocking::MockBlock<Test>;

pub const OWNER: u64 = 1;
pub const AGENT: u64 = 2;
pub const SHOP: u64 = 3;
pub const OTHER: u64 = 4;
pub const DEPOSIT_BASE: u64 = 10;
pub const DEPOSIT_PER_TARGET: u64 = 2;

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
	pub type Agents = pallet_agents;
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

/// Transfers are transfers. `transfer_all` stands for a contract call that moves value, and
/// `System::remark` for one that does not, to the account given by the first byte of the remark.
/// Everything else is forbidden.
pub struct Classifier;

impl ClassifyCall<RuntimeCall, u64> for Classifier {
	fn classify(call: &RuntimeCall) -> CallKind<u64> {
		match call {
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest, .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { dest, .. }) =>
				CallKind::Transfer { to: *dest },
			RuntimeCall::Balances(pallet_balances::Call::transfer_all { dest, .. }) =>
				CallKind::Contract { to: *dest },
			RuntimeCall::System(frame_system::Call::remark { remark }) =>
				CallKind::Contract { to: remark.first().copied().unwrap_or_default() as u64 },
			_ => CallKind::Forbidden,
		}
	}
}

impl pallet_agents::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type CallClassifier = Classifier;
	type MaxTargets = ConstU32<4>;
	type DepositBase = ConstU64<DEPOSIT_BASE>;
	type DepositPerTarget = ConstU64<DEPOSIT_PER_TARGET>;
	type WeightInfo = ();
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = Helper;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct Helper;

#[cfg(feature = "runtime-benchmarks")]
impl crate::BenchmarkHelper<RuntimeCall, u64> for Helper {
	fn transfer(to: u64) -> RuntimeCall {
		RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest: to, value: 10 })
	}
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(OWNER, 10_000), (AGENT, 100), (SHOP, 100), (OTHER, 100)],
		..Default::default()
	}
	.assimilate_storage(&mut storage)
	.unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| System::set_block_number(1));
	ext
}
