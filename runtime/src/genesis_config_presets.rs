use crate::{
	AccountId, AllocationsConfig, Balance, BalancesConfig, BlockNumber, Runtime, RuntimeGenesisConfig,
	TechnicalCommitteeConfig,
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

/// 32 bytes from their lowercase hexadecimal.
pub(crate) const fn bytes32(hex: &str) -> [u8; 32] {
	let hex = hex.as_bytes();
	let mut bytes = [0u8; 32];
	let mut i = 0;
	while i < 32 {
		bytes[i] = digit(hex[2 * i]) << 4 | digit(hex[2 * i + 1]);
		i += 1;
	}
	bytes
}

/// An account from its 32 bytes in lowercase hexadecimal.
const fn account(hex: &str) -> AccountId {
	AccountId::new(bytes32(hex))
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

/// Accounts of the public testnet, derived from the secret phrase of its operator: `//sudo`, the
/// key of sudo until governance replaced it, which funds the faucet; `//team`; and
/// `//foundation//1` to `//foundation//3`. They hold test TESS only.
mod testnet {
	use super::{account, AccountId};

	pub const OPERATOR: AccountId = account("7b0553fa3c7806743b6385729df590586d6f6597e25e763beb95f918e0d10385");
	pub const TEAM: AccountId = account("ff3b93268f3bacfa3622447c82adb7cf556f43c81ee2b78e95d58052fc89f8cc");
	pub const FOUNDATION: [AccountId; 3] = [
		account("585b1b19f28c02c672a580d78c337609fe416845d891544869c63624a6dd295f"),
		account("398f7778ccba60207fcc413f3433d4d815fb8c35589a8af586b3ac1a525dcc25"),
		account("b1426b054d11da66959059ee559a21462a7491dbdd69b70303183aa7f0d610de"),
	];
}

/// The technical committee of the public testnet: the signatories of its foundation.
pub fn testnet_committee() -> [AccountId; 3] {
	testnet::FOUNDATION
}

/// The technical committee of `members`, fixed at genesis.
fn committee(members: &[AccountId]) -> TechnicalCommitteeConfig {
	let mut members = members.to_vec();
	members.sort();
	TechnicalCommitteeConfig { members, ..Default::default() }
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

/// Development chains: the technical committee is Alice, Bob and Charlie, the signatories of the
/// development foundation.
fn testnet_genesis(endowed_accounts: Vec<AccountId>) -> Value {
	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig {
			balances: endowed_accounts.iter().cloned().map(|k| (k, 1_000_000 * UNIT)).collect::<Vec<_>>(),
		},
		allocations: AllocationsConfig { allocations: development_allocations() },
		difficulty: pallet_difficulty::GenesisConfig { initial_difficulty: U256::from(DEV_INITIAL_DIFFICULTY) },
		technical_committee: committee(&[Keyring::Alice, Keyring::Bob, Keyring::Charlie].map(|k| k.to_account_id())),
	})
}

/// Development chain: a single local miner.
pub fn development_config_genesis() -> Value {
	testnet_genesis(vec![Keyring::Alice.to_account_id(), Keyring::Bob.to_account_id()])
}

/// Local testnet: several miners on one machine or LAN.
pub fn local_config_genesis() -> Value {
	testnet_genesis(
		// Dave and Eve hold the development team allocation.
		[Keyring::Alice, Keyring::Bob, Keyring::Charlie, Keyring::Ferdie].map(|k| k.to_account_id()).to_vec(),
	)
}

/// Public testnet: the allocations of the mainnet, to test accounts, and test TESS for the faucet,
/// held by the operator. The testnet started with sudo; this preset describes its state since
/// governance replaced it, and gives another genesis than the one of the testnet.
pub fn testnet_config_genesis() -> Value {
	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig { balances: vec![(testnet::OPERATOR, 100_000 * UNIT)] },
		allocations: AllocationsConfig {
			allocations: allocations(&[(testnet::TEAM, TEAM_ALLOCATION)], foundation_account(&testnet::FOUNDATION, 2)),
		},
		difficulty: pallet_difficulty::GenesisConfig { initial_difficulty: U256::from(TESTNET_INITIAL_DIFFICULTY) },
		technical_committee: committee(&testnet::FOUNDATION),
	})
}

/// Mainnet: the team allocation shared by the two members of the team, the foundation allocation
/// to its 2-of-3 multisig, and nothing else: every other TESS is mined. There is no sudo key:
/// governance decides, and the technical committee is the three signatories of the foundation.
pub fn mainnet_config_genesis() -> Value {
	let foundation = foundation_account(&mainnet::FOUNDATION, 2);
	let team = mainnet::TEAM.map(|member| (member, TEAM_ALLOCATION / 2));
	build_struct_json_patch!(RuntimeGenesisConfig {
		allocations: AllocationsConfig { allocations: allocations(&team, foundation.clone()) },
		difficulty: pallet_difficulty::GenesisConfig { initial_difficulty: U256::from(MAINNET_INITIAL_DIFFICULTY) },
		technical_committee: committee(&mainnet::FOUNDATION),
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
