//! Block rewards against the full runtime: the miner is read from the proof-of-work digest.

use super::*;
use sp_runtime::{Digest, DigestItem};

/// Start block `number`, mined by `author` if any.
fn start_block(number: BlockNumber, author: Option<&Keyring>) {
	let mut digest = Digest::default();
	if let Some(author) = author {
		digest.push(DigestItem::PreRuntime(
			sp_consensus_pow::POW_ENGINE_ID,
			author.to_account_id().encode(),
		));
	}
	let header =
		Header::new(number, Default::default(), Default::default(), System::parent_hash(), digest);
	Executive::initialize_block(&header);
}

fn empty_ext() -> sp_io::TestExternalities {
	RuntimeGenesisConfig::default().build_storage().unwrap().into()
}

#[test]
fn miner_receives_the_block_reward() {
	empty_ext().execute_with(|| {
		let miner = Keyring::Alice;
		let issuance = Balances::total_issuance();
		start_block(1, Some(&miner));
		assert_eq!(Balances::balance(&miner.to_account_id()), 850 * MILLI_UNIT);
		assert_eq!(pallet_emission::TotalMinted::<Runtime>::get(), 850 * MILLI_UNIT);
		assert_eq!(Balances::total_issuance(), issuance + 850 * MILLI_UNIT);
	});
}

#[test]
fn blocks_without_miner_are_not_rewarded() {
	empty_ext().execute_with(|| {
		let issuance = Balances::total_issuance();
		start_block(1, None);
		assert_eq!(Balances::total_issuance(), issuance);
		assert_eq!(pallet_emission::TotalMinted::<Runtime>::get(), 0);
	});
}

#[test]
fn tessera_emission_schedule() {
	assert_eq!(Emission::block_reward(0), 0);
	assert_eq!(Emission::block_reward(1), 850 * MILLI_UNIT);
	assert_eq!(Emission::block_reward(HALVING_INTERVAL), 850 * MILLI_UNIT);
	assert_eq!(Emission::block_reward(HALVING_INTERVAL + 1), 425 * MILLI_UNIT);
	assert_eq!(Emission::block_reward(2 * HALVING_INTERVAL + 1), 212_500 * MICRO_UNIT);

	// The halvings mint at most the mining supply, 85% of 14 000 000 TESS.
	let scheduled = Emission::scheduled_supply();
	assert!(scheduled <= MINING_SUPPLY);
	assert!(MINING_SUPPLY - scheduled < MILLI_UNIT);
	assert_eq!(MINING_SUPPLY * 100, TOTAL_SUPPLY * 85);
}
