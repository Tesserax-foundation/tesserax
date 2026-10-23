//! Genesis allocations against the full runtime, built from the development preset.

use super::*;
use frame_support::{assert_ok, weights::Weight};
use frame_support::traits::fungible::Inspect;
use frame_support::traits::tokens::{Fortitude, Preservation};

/// Merge `patch` into `base`, as the node does with genesis presets.
fn merge(base: &mut serde_json::Value, patch: serde_json::Value) {
	match (base, patch) {
		(serde_json::Value::Object(base), serde_json::Value::Object(patch)) =>
			for (key, value) in patch {
				merge(base.entry(key).or_insert(serde_json::Value::Null), value);
			},
		(base, patch) => *base = patch,
	}
}

fn preset_ext() -> sp_io::TestExternalities {
	ext_of(sp_genesis_builder::DEV_RUNTIME_PRESET)
}

fn ext_of(preset: &str) -> sp_io::TestExternalities {
	let patch = genesis_config_presets::get_preset(&preset.into()).expect("the preset exists");
	let mut config = serde_json::to_value(RuntimeGenesisConfig::default()).unwrap();
	merge(&mut config, serde_json::from_slice(&patch).unwrap());
	let json = serde_json::to_vec(&config).unwrap();
	let mut ext = sp_io::TestExternalities::default();
	ext.execute_with(|| {
		frame_support::genesis_builder_helper::build_state::<RuntimeGenesisConfig>(json)
			.expect("the preset builds");
		System::set_block_number(1);
	});
	ext
}

fn spendable(who: &AccountId) -> Balance {
	Balances::reducible_balance(who, Preservation::Expendable, Fortitude::Polite)
}

fn foundation() -> AccountId {
	genesis_config_presets::foundation_account(
		&[Keyring::Alice, Keyring::Bob, Keyring::Charlie].map(|k| k.to_account_id()),
		2,
	)
}

#[test]
fn allocations_follow_the_tokenomics() {
	assert_eq!(TEAM_ALLOCATION * 10, TOTAL_SUPPLY);
	assert_eq!(FOUNDATION_ALLOCATION * 20, TOTAL_SUPPLY);
	assert_eq!(MINING_SUPPLY + TEAM_ALLOCATION + FOUNDATION_ALLOCATION, TOTAL_SUPPLY);
	assert_eq!(FOUNDATION_LIQUID, 140_000 * UNIT);
	// 48 and 36 months of 18 s blocks.
	assert_eq!(TEAM_VESTING, 7_012_800);
	assert_eq!(FOUNDATION_VESTING, 5_259_600);
	assert_eq!(TEAM_CLIFF, 876_600);
}

#[test]
fn development_genesis_creates_the_allocations() {
	preset_ext().execute_with(|| {
		let (dave, eve) = (Keyring::Dave.to_account_id(), Keyring::Eve.to_account_id());
		assert_eq!(Balances::balance(&dave), TEAM_ALLOCATION / 2);
		assert_eq!(Balances::balance(&eve), TEAM_ALLOCATION / 2);
		assert_eq!(Balances::balance(&foundation()), FOUNDATION_ALLOCATION);

		// The team cannot move anything before the cliff, the foundation only its liquid part.
		assert_eq!(spendable(&dave), 0);
		assert_eq!(spendable(&foundation()), FOUNDATION_LIQUID);
		assert!(pallet_allocations::Allocations::<Runtime>::get(&dave).is_some());
	});
}

#[test]
fn team_allocation_is_released_from_the_cliff() {
	preset_ext().execute_with(|| {
		let (alice, dave) = (Keyring::Alice.to_account_id(), Keyring::Dave.to_account_id());
		let share = TEAM_ALLOCATION / 2;

		System::set_block_number(TEAM_CLIFF - 1);
		assert_ok!(Vesting::vest_other(RuntimeOrigin::signed(alice.clone()), dave.clone().into()));
		assert_eq!(spendable(&dave), 0);

		// 12.5% at the cliff, released by any account since Dave cannot pay fees yet.
		System::set_block_number(TEAM_CLIFF);
		assert_ok!(Vesting::vest_other(RuntimeOrigin::signed(alice.clone()), dave.clone().into()));
		assert_eq!(spendable(&dave), share / 8);

		// Half after 24 months, everything after 48.
		System::set_block_number(24 * MONTH);
		assert_ok!(Vesting::vest(RuntimeOrigin::signed(dave.clone())));
		assert!(spendable(&dave).abs_diff(share / 2) < MILLI_UNIT);
		System::set_block_number(TEAM_VESTING);
		assert_ok!(Vesting::vest(RuntimeOrigin::signed(dave.clone())));
		assert_eq!(spendable(&dave), share);
	});
}

#[test]
fn foundation_spends_with_two_of_three_signatures() {
	preset_ext().execute_with(|| {
		let (alice, bob, charlie) =
			(Keyring::Alice.to_account_id(), Keyring::Bob.to_account_id(), Keyring::Charlie.to_account_id());
		let ferdie = Keyring::Ferdie.to_account_id();
		let call = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
			dest: ferdie.clone().into(),
			value: 1_000 * UNIT,
		});
		let weight = Weight::from_parts(1_000_000_000, 100_000);

		let mut others = vec![bob.clone(), charlie.clone()];
		others.sort();
		assert_ok!(Multisig::as_multi(
			RuntimeOrigin::signed(alice.clone()),
			2,
			others,
			None,
			Box::new(call.clone()),
			weight,
		));
		assert_eq!(Balances::balance(&ferdie), 0);

		let timepoint = pallet_multisig::Pallet::<Runtime>::timepoint();
		let mut others = vec![alice, charlie];
		others.sort();
		assert_ok!(Multisig::as_multi(
			RuntimeOrigin::signed(bob),
			2,
			others,
			Some(timepoint),
			Box::new(call),
			weight,
		));
		assert_eq!(Balances::balance(&ferdie), 1_000 * UNIT);
	});
}

#[test]
fn testnet_genesis_creates_the_allocations() {
	ext_of(genesis_config_presets::TESTNET_RUNTIME_PRESET).execute_with(|| {
		let sudo = pallet_sudo::Key::<Runtime>::get().expect("the testnet has a sudo key");
		assert_eq!(Balances::free_balance(&sudo), 100_000 * UNIT);
		// The faucet funds, the team and the foundation, and the existential deposits pallets create.
		let issued = 100_000 * UNIT + TEAM_ALLOCATION + FOUNDATION_ALLOCATION;
		assert!((issued..issued + UNIT).contains(&Balances::total_issuance()));
		assert_eq!(pallet_difficulty::CurrentDifficulty::<Runtime>::get(), sp_core::U256::from(1_000));
	});
}

