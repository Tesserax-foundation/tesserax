//! Block rewards and fees against the full runtime: the miner, read from the proof-of-work
//! digest, receives the reward and everything that would otherwise be burned.

use super::*;
use frame_support::assert_ok;
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
fn tesserax_emission_schedule() {
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

fn ext_with_accounts() -> sp_io::TestExternalities {
	RuntimeGenesisConfig {
		balances: pallet_balances::GenesisConfig {
			balances: vec![
				(Keyring::Alice.to_account_id(), 1_000 * UNIT),
				(Keyring::Bob.to_account_id(), 1_000 * UNIT),
			],
			..Default::default()
		},
		..Default::default()
	}
	.build_storage()
	.unwrap()
	.into()
}

#[test]
fn transaction_fees_go_to_the_miner() {
	ext_with_accounts().execute_with(|| {
		let (alice, miner) = (Keyring::Alice, Keyring::Charlie);
		let issuance = Balances::total_issuance();
		start_block(1, Some(&miner));

		let before = Balances::balance(&alice.to_account_id());
		let xt = signed(transfer_call(&Keyring::Bob, 10 * UNIT), alice.to_account_id(), &alice.pair(), 0);
		assert_eq!(apply(xt), Ok(Ok(())));
		let fee = before - Balances::balance(&alice.to_account_id()) - 10 * UNIT;
		assert!(fee > 0);

		assert_eq!(Balances::balance(&miner.to_account_id()), BLOCK_REWARD + fee);
		// Nothing is burned: only the block reward was created.
		assert_eq!(Balances::total_issuance(), issuance + BLOCK_REWARD);
	});
}

#[test]
fn dust_of_removed_accounts_goes_to_the_miner() {
	ext_with_accounts().execute_with(|| {
		let (alice, bob, miner) = (Keyring::Alice, Keyring::Bob, Keyring::Charlie);
		let issuance = Balances::total_issuance();
		start_block(1, Some(&miner));

		// Bob keeps half of the existential deposit: his account is removed with that dust.
		let dust = EXISTENTIAL_DEPOSIT / 2;
		let amount = Balances::balance(&bob.to_account_id()) - dust;
		assert_ok!(Balances::transfer_allow_death(
			RuntimeOrigin::signed(bob.to_account_id()),
			alice.to_account_id().into(),
			amount,
		));
		assert_eq!(Balances::balance(&bob.to_account_id()), 0);
		assert_eq!(Balances::balance(&miner.to_account_id()), BLOCK_REWARD + dust);
		assert_eq!(Balances::total_issuance(), issuance + BLOCK_REWARD);
	});
}
