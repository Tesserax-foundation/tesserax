use crate::{AccountId, BalancesConfig, RuntimeGenesisConfig, SudoConfig, UNIT};
use alloc::{vec, vec::Vec};
use frame_support::build_struct_json_patch;
use serde_json::Value;
use sp_core::U256;
use sp_genesis_builder::{self, PresetId};
use tessera_crypto::keyring::Keyring;

/// Initial difficulty of development chains: a few seconds per block for a single CPU core.
const DEV_INITIAL_DIFFICULTY: u64 = 2_000;

fn testnet_genesis(endowed_accounts: Vec<AccountId>, root: AccountId) -> Value {
	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig {
			balances: endowed_accounts
				.iter()
				.cloned()
				.map(|k| (k, 1_000_000 * UNIT))
				.collect::<Vec<_>>(),
		},
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
		Keyring::account_ids(),
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
