use crate::{mock::*, next_difficulty, CurrentDifficulty, History};
use sp_core::U256;

fn difficulty() -> u64 {
	CurrentDifficulty::<Test>::get().as_u64()
}

fn ratio(a: u64, b: u64) -> f64 {
	a as f64 / b as f64
}

/// Mine `blocks` blocks with a hashrate that matches `hashrate_difficulty` at the target block
/// time. Solve times follow the current difficulty, without randomness.
fn mine_with_hashrate(hashrate_difficulty: u64, blocks: u32) {
	for _ in 0..blocks {
		let solve_time =
			(TARGET as u128 * difficulty() as u128 / hashrate_difficulty as u128) as u64;
		produce_block(solve_time.max(1));
	}
}

#[test]
fn genesis_sets_initial_difficulty() {
	new_test_ext().execute_with(|| {
		assert_eq!(difficulty(), INITIAL);
		assert!(History::<Test>::get().is_empty());
	});
}

#[test]
fn needs_two_blocks_to_adjust() {
	assert_eq!(next_difficulty(&[], TARGET, U256::one()), None);
	assert_eq!(next_difficulty(&[(0, U256::from(5))], TARGET, U256::one()), None);
}

#[test]
fn stable_when_on_target() {
	new_test_ext().execute_with(|| {
		for _ in 0..300 {
			produce_block(TARGET);
		}
		assert_eq!(difficulty(), INITIAL);
	});
}

#[test]
fn history_is_bounded() {
	new_test_ext().execute_with(|| {
		for _ in 0..(WINDOW * 3) {
			produce_block(TARGET);
		}
		assert_eq!(History::<Test>::get().len(), WINDOW as usize + 1);
	});
}

#[test]
fn follows_hashrate_increase() {
	new_test_ext().execute_with(|| {
		mine_with_hashrate(INITIAL, 50);
		mine_with_hashrate(2 * INITIAL, WINDOW);
		let r = ratio(difficulty(), 2 * INITIAL);
		assert!(r > 0.9 && r < 1.1, "ratio {r}");
	});
}

#[test]
fn follows_hashrate_drop() {
	new_test_ext().execute_with(|| {
		mine_with_hashrate(INITIAL, 50);
		mine_with_hashrate(INITIAL / 2, WINDOW);
		let r = ratio(difficulty(), INITIAL / 2);
		assert!(r > 0.9 && r < 1.1, "ratio {r}");
	});
}

#[test]
fn converges_to_hashrate() {
	new_test_ext().execute_with(|| {
		mine_with_hashrate(4 * INITIAL, WINDOW * 4);
		let r = ratio(difficulty(), 4 * INITIAL);
		assert!(r > 0.97 && r < 1.03, "ratio {r}");
	});
}

#[test]
fn single_future_timestamp_has_bounded_effect() {
	new_test_ext().execute_with(|| {
		for _ in 0..WINDOW {
			produce_block(TARGET);
		}
		// A block claiming to be one day late counts as six target times at most.
		produce_block(86_400_000);
		let r = ratio(difficulty(), INITIAL);
		assert!(r > 0.9, "ratio {r}");
	});
}

#[test]
fn increase_is_bounded() {
	let history: Vec<_> = (0..11u64).map(|i| (i, U256::from(INITIAL))).collect();
	let next = next_difficulty(&history, TARGET, U256::one()).unwrap();
	assert_eq!(next, U256::from(INITIAL * 10));
}

#[test]
fn never_below_minimum() {
	new_test_ext().execute_with(|| {
		for _ in 0..(WINDOW * 4) {
			produce_block(TARGET * 6);
		}
		assert_eq!(difficulty(), MINIMUM);
	});
}
