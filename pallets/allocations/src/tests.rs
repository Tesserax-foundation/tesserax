use crate::{mock::*, Allocation, Allocations, Schedule};
use frame_support::{assert_noop, assert_ok};

/// Spendable balance of `who` at block `n`, after releasing what has vested.
fn spendable_at(who: u64, n: u64) -> u64 {
	System::set_block_number(n);
	let _ = Vesting::vest(RuntimeOrigin::signed(who));
	Balances::usable_balance(who)
}

#[test]
fn allocations_are_credited_and_recorded() {
	new_test_ext().execute_with(|| {
		assert_eq!(Balances::free_balance(TEAM), TEAM_AMOUNT);
		assert_eq!(Balances::free_balance(FOUNDATION), FOUNDATION_AMOUNT);
		assert_eq!(Balances::total_issuance(), 100 + TEAM_AMOUNT + FOUNDATION_AMOUNT);
		assert_eq!(
			Allocations::<Test>::get(TEAM),
			Some(Allocation { amount: TEAM_AMOUNT, liquid: 0, cliff: TEAM_CLIFF, duration: TEAM_DURATION })
		);
	});
}

#[test]
fn team_is_locked_until_the_cliff_then_vests_linearly() {
	new_test_ext().execute_with(|| {
		// Nothing can be moved before the cliff.
		assert_eq!(spendable_at(TEAM, 1), 0);
		assert_eq!(spendable_at(TEAM, TEAM_CLIFF - 1), 0);
		assert_noop!(
			Balances::transfer_allow_death(RuntimeOrigin::signed(TEAM), OTHER, 1),
			sp_runtime::TokenError::Frozen
		);

		// At the cliff, the six months accrued since genesis are released at once: 12.5%.
		assert_eq!(spendable_at(TEAM, TEAM_CLIFF), TEAM_AMOUNT * TEAM_CLIFF / TEAM_DURATION);
		assert_eq!(spendable_at(TEAM, TEAM_CLIFF), 600);

		// Then linearly: 100 per block here.
		assert_eq!(spendable_at(TEAM, 7), 700);
		assert_eq!(spendable_at(TEAM, 24), 2_400);
		assert_eq!(spendable_at(TEAM, TEAM_DURATION - 1), 4_700);
		assert_eq!(spendable_at(TEAM, TEAM_DURATION), TEAM_AMOUNT);
		assert_eq!(spendable_at(TEAM, 1_000), TEAM_AMOUNT);

		assert_ok!(Balances::transfer_allow_death(RuntimeOrigin::signed(TEAM), OTHER, TEAM_AMOUNT));
	});
}

#[test]
fn foundation_has_a_liquid_part_and_vests_without_cliff() {
	new_test_ext().execute_with(|| {
		assert_eq!(spendable_at(FOUNDATION, 0), FOUNDATION_LIQUID);
		assert_ok!(Balances::transfer_allow_death(RuntimeOrigin::signed(FOUNDATION), OTHER, 140));
		assert_noop!(
			Balances::transfer_allow_death(RuntimeOrigin::signed(FOUNDATION), OTHER, 1),
			sp_runtime::TokenError::Frozen
		);

		// 560 over 36 blocks, rounded up: 16 per block.
		assert_eq!(spendable_at(FOUNDATION, 1), 16);
		assert_eq!(spendable_at(FOUNDATION, 18), 288);
		assert_eq!(spendable_at(FOUNDATION, FOUNDATION_DURATION), 560);
	});
}

#[test]
fn anyone_can_release_vested_funds() {
	new_test_ext().execute_with(|| {
		// The team holds nothing spendable at the cliff: another account releases its funds.
		System::set_block_number(TEAM_CLIFF);
		assert_eq!(Balances::usable_balance(TEAM), 0);
		assert_ok!(Vesting::vest_other(RuntimeOrigin::signed(OTHER), TEAM));
		assert_eq!(Balances::usable_balance(TEAM), 600);
		assert_ok!(Balances::transfer_allow_death(RuntimeOrigin::signed(TEAM), OTHER, 600));
	});
}

#[test]
fn schedules_release_exactly_the_vested_part() {
	type A = Allocation<u128, u32>;
	let unit: u128 = 1_000_000_000_000;
	let month: u32 = 146_100;
	let team = A { amount: 1_400_000 * unit, liquid: 0, cliff: 6 * month, duration: 48 * month };
	let foundation =
		A { amount: 700_000 * unit, liquid: 140_000 * unit, cliff: 0, duration: 36 * month };

	let locked_at = |schedules: &[Schedule<u128, u32>], n: u32| -> u128 {
		schedules
			.iter()
			.map(|s| s.locked.saturating_sub(s.per_block * n.saturating_sub(s.start) as u128))
			.sum()
	};

	let s = team.schedules();
	assert_eq!(s.len(), 2);
	assert_eq!(locked_at(&s, 0), team.amount);
	assert_eq!(locked_at(&s, 6 * month - 1), team.amount);
	assert_eq!(team.amount - locked_at(&s, 6 * month), 175_000 * unit);
	let half = team.amount - locked_at(&s, 24 * month);
	assert!(half.abs_diff(700_000 * unit) < unit / 1_000);
	assert_eq!(locked_at(&s, 48 * month), 0);
	assert!(locked_at(&s, 48 * month - 1) > 0);

	let s = foundation.schedules();
	assert_eq!(s.len(), 1);
	assert_eq!(locked_at(&s, 0), 560_000 * unit);
	assert_eq!(locked_at(&s, 36 * month), 0);
	assert!(locked_at(&s, 36 * month - 1) > 0);

	// Fully liquid, or released at once at the cliff.
	assert!(A { amount: 5, liquid: 5, cliff: 0, duration: 0 }.schedules().is_empty());
	assert_eq!(
		A { amount: 10, liquid: 0, cliff: 4, duration: 4 }.schedules(),
		vec![Schedule { locked: 10, per_block: 10, start: 3 }]
	);
}

#[test]
fn invalid_allocations_are_detected() {
	type A = Allocation<u64, u64>;
	assert!(!A { amount: 0, liquid: 0, cliff: 0, duration: 1 }.is_valid());
	assert!(!A { amount: 10, liquid: 11, cliff: 0, duration: 1 }.is_valid());
	assert!(!A { amount: 10, liquid: 0, cliff: 5, duration: 4 }.is_valid());
	assert!(!A { amount: 10, liquid: 0, cliff: 0, duration: 0 }.is_valid());
	assert!(A { amount: 10, liquid: 10, cliff: 0, duration: 0 }.is_valid());
}

#[test]
#[should_panic(expected = "duplicate allocation")]
fn duplicate_allocations_are_rejected() {
	use sp_runtime::BuildStorage;
	pallet_allocations::GenesisConfig::<Test> {
		allocations: vec![(TEAM, 100, 0, 0, 10), (TEAM, 100, 0, 0, 10)],
	}
	.build_storage()
	.unwrap();
}

use crate as pallet_allocations;
