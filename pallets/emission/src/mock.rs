use crate as pallet_emission;
use codec::Decode;
use frame_support::{
	derive_impl,
	traits::{ConstU64, FindAuthor},
};
use sp_runtime::{BuildStorage, ConsensusEngineId, Digest, DigestItem};

type Block = frame_system::mocking::MockBlock<Test>;

/// Engine identifier of the author digest.
pub const ENGINE_ID: ConsensusEngineId = *b"pow_";
/// Existing account of a miner.
pub const MINER: u64 = 1;
pub const EXISTENTIAL_DEPOSIT: u64 = 10;
pub const INITIAL_REWARD: u64 = 850;
pub const HALVING_INTERVAL: u64 = 100;
pub const MINING_SUPPLY: u64 = 2 * INITIAL_REWARD * HALVING_INTERVAL;

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
	pub type Emission = pallet_emission;
}

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountData = pallet_balances::AccountData<u64>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type AccountStore = System;
	type ExistentialDeposit = ConstU64<EXISTENTIAL_DEPOSIT>;
}

/// The author is the account encoded in the digest of [`ENGINE_ID`], as in the runtime.
pub struct DigestAuthor;

impl FindAuthor<u64> for DigestAuthor {
	fn find_author<'a, I>(digests: I) -> Option<u64>
	where
		I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
	{
		digests.into_iter().find_map(|(id, mut data)| {
			(id == ENGINE_ID).then(|| u64::decode(&mut data).ok())?
		})
	}
}

impl pallet_emission::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type FindAuthor = DigestAuthor;
	type InitialReward = ConstU64<INITIAL_REWARD>;
	type HalvingInterval = ConstU64<HALVING_INTERVAL>;
	type MiningSupply = ConstU64<MINING_SUPPLY>;
	type WeightInfo = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> { balances: vec![(MINER, 100)], ..Default::default() }
		.assimilate_storage(&mut storage)
		.unwrap();
	sp_io::TestExternalities::new(storage)
}

/// Start block `n`, mined by `author` if any.
pub fn start_block(n: u64, author: Option<u64>) {
	let mut digest = Digest::default();
	if let Some(author) = author {
		digest.push(DigestItem::PreRuntime(ENGINE_ID, codec::Encode::encode(&author)));
	}
	System::reset_events();
	System::set_block_number(n - 1);
	System::initialize(&n, &Default::default(), &digest);
	<Emission as frame_support::traits::Hooks<u64>>::on_initialize(n);
}
