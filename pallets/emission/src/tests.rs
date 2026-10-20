use crate::{mock::*, Event, TotalMinted};

fn reward(n: u64) -> u64 {
	Emission::block_reward(n)
}

#[test]
fn reward_halves_every_interval() {
	new_test_ext().execute_with(|| {
		assert_eq!(reward(0), 0);
		assert_eq!(reward(1), INITIAL_REWARD);
		assert_eq!(reward(HALVING_INTERVAL), INITIAL_REWARD);
		assert_eq!(reward(HALVING_INTERVAL + 1), INITIAL_REWARD / 2);
		assert_eq!(reward(2 * HALVING_INTERVAL), INITIAL_REWARD / 2);
		assert_eq!(reward(2 * HALVING_INTERVAL + 1), INITIAL_REWARD / 4);
		assert_eq!(reward(10 * HALVING_INTERVAL + 1), 0);
		assert_eq!(reward(u64::MAX), 0);
	});
}

#[test]
fn miner_is_rewarded() {
	new_test_ext().execute_with(|| {
		start_block(1, Some(MINER));
		assert_eq!(Balances::free_balance(MINER), 100 + INITIAL_REWARD);
		assert_eq!(TotalMinted::<Test>::get(), INITIAL_REWARD);
		assert_eq!(Balances::total_issuance(), 100 + INITIAL_REWARD);
		System::assert_last_event(Event::Rewarded { author: MINER, amount: INITIAL_REWARD }.into());

		// A new account is created by its first reward.
		start_block(2, Some(7));
		assert_eq!(Balances::free_balance(7), INITIAL_REWARD);
		assert_eq!(TotalMinted::<Test>::get(), 2 * INITIAL_REWARD);
	});
}

#[test]
fn blocks_without_author_are_not_rewarded() {
	new_test_ext().execute_with(|| {
		start_block(1, None);
		assert_eq!(TotalMinted::<Test>::get(), 0);
		assert_eq!(Balances::total_issuance(), 100);
		assert!(System::events().is_empty());
	});
}

#[test]
fn rewards_below_the_existential_deposit_of_a_new_account_are_not_minted() {
	new_test_ext().execute_with(|| {
		// 850 >> 7 = 6, below the existential deposit of 10.
		let n = 7 * HALVING_INTERVAL + 1;
		assert_eq!(reward(n), 6);
		start_block(n, Some(7));
		assert_eq!(Balances::free_balance(7), 0);
		assert_eq!(TotalMinted::<Test>::get(), 0);

		// An existing account still receives it.
		start_block(n + 1, Some(MINER));
		assert_eq!(Balances::free_balance(MINER), 106);
		assert_eq!(TotalMinted::<Test>::get(), 6);
	});
}

#[test]
fn rewards_stop_at_the_mining_supply() {
	new_test_ext().execute_with(|| {
		TotalMinted::<Test>::put(MINING_SUPPLY - 100);
		start_block(1, Some(MINER));
		assert_eq!(TotalMinted::<Test>::get(), MINING_SUPPLY);
		assert_eq!(Balances::free_balance(MINER), 200);
		start_block(2, Some(MINER));
		assert_eq!(TotalMinted::<Test>::get(), MINING_SUPPLY);
		assert_eq!(Balances::free_balance(MINER), 200);
	});
}

#[test]
fn whole_schedule_mints_the_scheduled_supply() {
	new_test_ext().execute_with(|| {
		let last = 12 * HALVING_INTERVAL;
		for n in 1..=last {
			start_block(n, Some(MINER));
		}
		let scheduled = Emission::scheduled_supply();
		assert_eq!(TotalMinted::<Test>::get(), scheduled);
		assert!(scheduled <= MINING_SUPPLY);
		// 850 + 425 + 212 + 106 + 53 + 26 + 13 + 6 + 3 + 1 per block of each era.
		assert_eq!(scheduled, 1695 * HALVING_INTERVAL);
	});
}

#[test]
fn tesserax_schedule() {
	// 0.85 TESS per block, halving every 7 000 000 blocks: 11 900 000 TESS at most.
	const UNIT: u128 = 1_000_000_000_000;
	let initial = 85 * UNIT / 100;
	let total: u128 = (0..128).map(|h| (initial >> h) * 7_000_000).sum();
	assert!(total <= 11_900_000 * UNIT);
	assert!(11_900_000 * UNIT - total < UNIT / 10);
}
