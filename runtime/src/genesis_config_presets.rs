use crate::{
	AccountId, AllocationsConfig, Balance, BalancesConfig, BlockNumber, Runtime, RuntimeGenesisConfig, SudoConfig,
	FOUNDATION_ALLOCATION, FOUNDATION_LIQUID, FOUNDATION_VESTING, TEAM_ALLOCATION, TEAM_CLIFF, TEAM_VESTING, UNIT,
};
use alloc::{vec, vec::Vec};
use frame_support::build_struct_json_patch;
use serde_json::Value;
use sp_core::U256;
use sp_genesis_builder::{self, PresetId};
use tesserax_crypto::keyring::Keyring;

/// Initial difficulty of development chains: a few seconds per block for a single CPU core.
const DEV_INITIAL_DIFFICULTY: u64 = 2_000;

/// Preset of the public testnet.
pub const TESTNET_RUNTIME_PRESET: &str = "testnet";

/// Initial difficulty of the public testnet: about a block per 18 seconds for one CPU core mining
/// with the light dataset, the miner the network starts with. It follows the miners who join.
const TESTNET_INITIAL_DIFFICULTY: u64 = 1_000;

/// Accounts of the public testnet, derived from the secret phrase of its operator: `//sudo`,
/// `//team` and `//foundation//1` to `//foundation//3`. They hold test TESS only.
/// An account from its 32 bytes in lowercase hexadecimal.
const fn account(hex: &str) -> AccountId {
	let hex = hex.as_bytes();
	let mut bytes = [0u8; 32];
	let mut i = 0;
	while i < 32 {
		bytes[i] = digit(hex[2 * i]) << 4 | digit(hex[2 * i + 1]);
		i += 1;
	}
	AccountId::new(bytes)
}

const fn digit(c: u8) -> u8 {
	match c {
		b'0'..=b'9' => c - b'0',
		b'a'..=b'f' => c - b'a' + 10,
		_ => panic!("lowercase hexadecimal"),
	}
}

/// Preset of the mainnet.
pub const MAINNET_RUNTIME_PRESET: &str = "mainnet";

/// Initial difficulty of the mainnet: a block per 18 seconds at about 55 kH/s, a dozen CPUs mining
/// with XMRig. Too low, the first blocks would come in seconds to the first miners until the
/// difficulty catches up; too high, they come slowly until it comes down. Set it from the
/// hashrate of the testnet just before the launch.
const MAINNET_INITIAL_DIFFICULTY: u64 = 1_000_000;

/// Accounts of the mainnet genesis.
mod mainnet {
	use super::{account, AccountId};

	/// tetTtAxXxBzRTTsc5otocLgRrGZVCcxzBeK4NSGwdqAsee7XM
	const MEMBER_1: AccountId = account("f431a9b4f3b863a7e6db10044649f0faab5a892c8c4d564df4bea4be744ba353");
	/// tetBiN5GwskQqq8ykcMUQbqCPdWPRaE5PoYwfazsJ2jHGtTZw
	const MEMBER_2: AccountId = account("e7dcb7f810e080ca7d9359be9ff13bab01e785c3b76c3f54f47c0fc8134e789c");
	/// terN4gSZpiFpeERvb1PoaJiiBRqSEmXw56xF5MqqjLbp7eH3x, a backup key kept offline.
	const BACKUP: AccountId = account("9749190899187e80c962c0e9854dee8de513b4e309055e57b95b0749c351d30a");

	/// The team, sharing its allocation equally.
	pub const TEAM: [AccountId; 2] = [MEMBER_1, MEMBER_2];

	/// Signatories of the foundation, two of three approving: the two members of the team and the
	/// backup key.
	pub const FOUNDATION: [AccountId; 3] = [MEMBER_1, MEMBER_2, BACKUP];
}

mod testnet {
	use super::{account, AccountId};

	pub const SUDO: AccountId = account("7b0553fa3c7806743b6385729df590586d6f6597e25e763beb95f918e0d10385");
	pub const TEAM: AccountId = account("ff3b93268f3bacfa3622447c82adb7cf556f43c81ee2b78e95d58052fc89f8cc");
	pub const FOUNDATION: [AccountId; 3] = [
		account("585b1b19f28c02c672a580d78c337609fe416845d891544869c63624a6dd295f"),
		account("398f7778ccba60207fcc413f3433d4d815fb8c35589a8af586b3ac1a525dcc25"),
		account("b1426b054d11da66959059ee559a21462a7491dbdd69b70303183aa7f0d610de"),
	];
}

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
	let team =
		[(Keyring::Dave.to_account_id(), TEAM_ALLOCATION / 2), (Keyring::Eve.to_account_id(), TEAM_ALLOCATION / 2)];
	let foundation =
		foundation_account(&[Keyring::Alice, Keyring::Bob, Keyring::Charlie].map(|k| k.to_account_id()), 2);
	allocations(&team, foundation)
}

fn testnet_genesis(endowed_accounts: Vec<AccountId>, root: AccountId) -> Value {
	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig {
			balances: endowed_accounts.iter().cloned().map(|k| (k, 1_000_000 * UNIT)).collect::<Vec<_>>(),
		},
		allocations: AllocationsConfig { allocations: development_allocations() },
		difficulty: pallet_difficulty::GenesisConfig { initial_difficulty: U256::from(DEV_INITIAL_DIFFICULTY) },
		sudo: SudoConfig { key: Some(root) },
	})
}

/// Development chain: a single local miner.
pub fn development_config_genesis() -> Value {
	testnet_genesis(vec![Keyring::Alice.to_account_id(), Keyring::Bob.to_account_id()], Keyring::Alice.to_account_id())
}

/// Local testnet: several miners on one machine or LAN.
pub fn local_config_genesis() -> Value {
	testnet_genesis(
		// Dave and Eve hold the development team allocation.
		[Keyring::Alice, Keyring::Bob, Keyring::Charlie, Keyring::Ferdie].map(|k| k.to_account_id()).to_vec(),
		Keyring::Alice.to_account_id(),
	)
}

/// Public testnet: the allocations of the mainnet, to test accounts, and test TESS for the faucet,
/// held by the sudo key.
pub fn testnet_config_genesis() -> Value {
	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig { balances: vec![(testnet::SUDO, 100_000 * UNIT)] },
		allocations: AllocationsConfig {
			allocations: allocations(&[(testnet::TEAM, TEAM_ALLOCATION)], foundation_account(&testnet::FOUNDATION, 2)),
		},
		difficulty: pallet_difficulty::GenesisConfig { initial_difficulty: U256::from(TESTNET_INITIAL_DIFFICULTY) },
		sudo: SudoConfig { key: Some(testnet::SUDO) },
	})
}

/// Mainnet: the team allocation shared by the two members of the team, the foundation allocation
/// to its 2-of-3 multisig, which is also the sudo key, and nothing else: every other TESS is mined.
pub fn mainnet_config_genesis() -> Value {
	let foundation = foundation_account(&mainnet::FOUNDATION, 2);
	let team = mainnet::TEAM.map(|member| (member, TEAM_ALLOCATION / 2));
	build_struct_json_patch!(RuntimeGenesisConfig {
		allocations: AllocationsConfig { allocations: allocations(&team, foundation.clone()) },
		difficulty: pallet_difficulty::GenesisConfig { initial_difficulty: U256::from(MAINNET_INITIAL_DIFFICULTY) },
		sudo: SudoConfig { key: Some(foundation) },
	})
}

/// The JSON genesis config patch of the preset `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::DEV_RUNTIME_PRESET => development_config_genesis(),
		sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET => local_config_genesis(),
		TESTNET_RUNTIME_PRESET => testnet_config_genesis(),
		MAINNET_RUNTIME_PRESET => mainnet_config_genesis(),
		_ => return None,
	};
	Some(serde_json::to_string(&patch).expect("serialization to json is expected to work. qed.").into_bytes())
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
	vec![
		PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
		PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
		PresetId::from(TESTNET_RUNTIME_PRESET),
		PresetId::from(MAINNET_RUNTIME_PRESET),
	]
}
