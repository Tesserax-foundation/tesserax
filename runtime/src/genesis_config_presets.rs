use crate::{
	AccountId, AllocationsConfig, Balance, BalancesConfig, BlockNumber, Runtime,
	RuntimeGenesisConfig, SudoConfig, FOUNDATION_ALLOCATION, FOUNDATION_LIQUID, FOUNDATION_VESTING,
	TEAM_ALLOCATION, TEAM_CLIFF, TEAM_VESTING, UNIT,
};
use alloc::{vec, vec::Vec};
use frame_support::build_struct_json_patch;
use serde_json::Value;
use sp_core::U256;
use sp_genesis_builder::{self, PresetId};
use tessera_crypto::keyring::Keyring;

/// Initial difficulty of development chains: a few seconds per block for a single CPU core.
const DEV_INITIAL_DIFFICULTY: u64 = 2_000;

/// Account of the foundation: a multisig of `signatories` requiring `threshold` approvals.
pub fn foundation_account(signatories: &[AccountId], threshold: u16) -> AccountId {
	let mut signatories = signatories.to_vec();
	signatories.sort();
	signatories.dedup();
	assert!(
		threshold >= 2 && usize::from(threshold) <= signatories.len(),
		"the foundation needs at least two approvals among its signatories"
	);
	pallet_multisig::Pallet::<Runtime>::multi_account_id(&signatories, threshold)
}

/// Genesis allocations: the team shares 10% of the supply, with a six-month cliff and 48 months
/// of vesting; the foundation receives 5%, a fifth of it liquid and the rest vested over 36
/// months. Returns `(beneficiary, amount, liquid, cliff, duration)`.
pub fn allocations(
	team: &[(AccountId, Balance)],
	foundation: AccountId,
) -> Vec<(AccountId, Balance, Balance, BlockNumber, BlockNumber)> {
	assert_eq!(
		team.iter().map(|(_, amount)| amount).sum::<Balance>(),
		TEAM_ALLOCATION,
		"the team shares must add up to the team allocation"
	);
	team.iter()
		.map(|(who, amount)| (who.clone(), *amount, 0, TEAM_CLIFF, TEAM_VESTING))
		.chain([(foundation, FOUNDATION_ALLOCATION, FOUNDATION_LIQUID, 0, FOUNDATION_VESTING)])
		.collect()
}

/// Development allocations: the team is Dave and Eve, the foundation a 2-of-3 multisig of
/// Alice, Bob and Charlie.
pub fn development_allocations() -> Vec<(AccountId, Balance, Balance, BlockNumber, BlockNumber)> {
	let team = [
		(Keyring::Dave.to_account_id(), TEAM_ALLOCATION / 2),
		(Keyring::Eve.to_account_id(), TEAM_ALLOCATION / 2),
	];
	let foundation = foundation_account(
		&[Keyring::Alice, Keyring::Bob, Keyring::Charlie].map(|k| k.to_account_id()),
		2,
	);
	allocations(&team, foundation)
}

fn testnet_genesis(endowed_accounts: Vec<AccountId>, root: AccountId) -> Value {
	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig {
			balances: endowed_accounts
				.iter()
				.cloned()
				.map(|k| (k, 1_000_000 * UNIT))
				.collect::<Vec<_>>(),
		},
		allocations: AllocationsConfig { allocations: development_allocations() },
		difficulty: pallet_difficulty::GenesisConfig {
			initial_difficulty: U256::from(DEV_INITIAL_DIFFICULTY),
		},
		sudo: SudoConfig { key: Some(root) },
	})
}

/// Development chain: a single local miner.
pub fn development_config_genesis() -> Value {
	testnet_genesis(
		vec![Keyring::Alice.to_account_id(), Keyring::Bob.to_account_id()],
		Keyring::Alice.to_account_id(),
	)
}

/// Local testnet: several miners on one machine or LAN.
pub fn local_config_genesis() -> Value {
	testnet_genesis(
		// Dave and Eve hold the development team allocation.
		[Keyring::Alice, Keyring::Bob, Keyring::Charlie, Keyring::Ferdie]
			.map(|k| k.to_account_id())
			.to_vec(),
		Keyring::Alice.to_account_id(),
	)
}

/// The JSON genesis config patch of the preset `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::DEV_RUNTIME_PRESET => development_config_genesis(),
		sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET => local_config_genesis(),
		_ => return None,
	};
	Some(
		serde_json::to_string(&patch)
			.expect("serialization to json is expected to work. qed.")
			.into_bytes(),
	)
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
	vec![
		PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
		PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
	]
}
