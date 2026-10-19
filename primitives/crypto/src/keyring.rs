//! Well-known development accounts.
//!
//! Each account is derived from the public development phrase with a hard junction named after
//! it (`//Alice`, `//Bob`, ...). These keys are public: never use them outside development and
//! test networks.

use crate::{Pair, Public};
use alloc::vec::Vec;
use sp_core::crypto::Pair as _;
use sp_runtime::AccountId32;

/// Development accounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Keyring {
	Alice,
	Bob,
	Charlie,
	Dave,
	Eve,
	Ferdie,
}

impl Keyring {
	/// All development accounts.
	pub fn iter() -> impl Iterator<Item = Keyring> {
		[Self::Alice, Self::Bob, Self::Charlie, Self::Dave, Self::Eve, Self::Ferdie].into_iter()
	}

	/// Name of the account, also its derivation junction.
	pub fn name(self) -> &'static str {
		match self {
			Self::Alice => "Alice",
			Self::Bob => "Bob",
			Self::Charlie => "Charlie",
			Self::Dave => "Dave",
			Self::Eve => "Eve",
			Self::Ferdie => "Ferdie",
		}
	}

	/// Secret URI of the account.
	pub fn uri(self) -> alloc::string::String {
		alloc::format!("//{}", self.name())
	}

	/// Key pair of the account.
	pub fn pair(self) -> Pair {
		Pair::from_string(&self.uri(), None).expect("development URIs are valid; qed")
	}

	/// Public key of the account.
	pub fn public(self) -> Public {
		self.pair().public()
	}

	/// Account id of the account.
	pub fn to_account_id(self) -> AccountId32 {
		self.pair().account_id()
	}

	/// Account ids of all development accounts.
	pub fn account_ids() -> Vec<AccountId32> {
		Self::iter().map(Self::to_account_id).collect()
	}
}
